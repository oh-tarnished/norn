//! Turning a refused batch into an error a person can act on.

use norn_protocol::common::Sample;
use norn_protocol::telemetry::{AppendSamplesResponse, BackpressureReason};

use crate::{AdapterError, time};

/// How many bad samples an error names. A batch can have thousands.
const NAMED: usize = 3;

/// The refusal, naming the first few bad samples so a user can find them.
pub(super) fn refused(reply: &AppendSamplesResponse, samples: &[Sample]) -> AdapterError {
    let admission = reply.admission.clone().unwrap_or_default();
    let reason = BackpressureReason::try_from(admission.reason);
    let reason = reason.map_or_else(
        |_| admission.reason.to_string(),
        |r| r.as_str_name().to_owned(),
    );
    let mut detail =
        (reply.error.as_ref()).map_or_else(|| admission.message.clone(), |e| e.message.clone());
    for error in reply.sample_errors.iter().take(NAMED) {
        let index = usize::try_from(error.index).ok();
        let sample = index.and_then(|i| samples.get(i));
        let text = error
            .status
            .as_ref()
            .map_or("invalid", |s| s.message.as_str());
        if let Some(sample) = sample {
            let at = sample.observe_time.as_ref().and_then(time::to_nanos);
            let at = at.map_or_else(|| "no time".to_owned(), time::format_rfc3339);
            detail.push_str(&format!("; {} at {at}: {text}", sample.signal));
        }
    }
    AdapterError::Refused { reason, detail }
}
