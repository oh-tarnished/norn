//! `nornctl state`: `ComputeState`, every page of it.

use std::io::Write;

use norn_protocol::state::ComputeStateRequest;
use norn_protocol::state::state_service_client::StateServiceClient;
use tonic::transport::Channel;

use crate::CliError;
use crate::query::{print_sample, selection, timestamp, window};
use crate::resources::{call, usage};
use crate::time::format;

fn request(words: &[&str]) -> Result<ComputeStateRequest, CliError> {
    let Some((target, flags)) = words.split_first() else {
        return Err(usage("state TARGET [--at T] [--max-age D] [--select F]"));
    };
    let (parent, filter) = selection(target);
    let mut r = ComputeStateRequest {
        parent,
        filter,
        state_time: Some(timestamp("now")?),
        ..ComputeStateRequest::default()
    };
    let mut flags = flags.iter();
    while let Some(flag) = flags.next() {
        let word = *flags.next().ok_or_else(|| usage("a flag needs a value"))?;
        match *flag {
            "--at" => r.state_time = Some(timestamp(word)?),
            "--max-age" => r.max_age = Some(window(word)?),
            "--select" => r.filter = word.to_owned(),
            _ => return Err(usage("state: unknown flag (see --help)")),
        }
    }
    Ok(r)
}

/// Prints the state at an instant: `SIGNAL unknown` for a signal with none.
pub(crate) async fn state(
    channel: Channel,
    words: &[&str],
    out: &mut impl Write,
) -> Result<(), CliError> {
    let mut request = request(words)?;
    let mut client = StateServiceClient::new(channel);
    let mut first = true;
    loop {
        let page = client.compute_state(request.clone()).await;
        let page = page
            .map_err(call("StateService.ComputeState"))?
            .into_inner();
        if let (true, Some(t)) = (first, page.state_time) {
            writeln!(out, "state_time: {}", format(t.seconds, t.nanos))?;
            first = false;
        }
        for entry in page.signal_states {
            match entry.sample {
                Some(sample) => print_sample(out, sample)?,
                None => writeln!(out, "- {} unknown", entry.signal)?,
            }
        }
        if page.next_page_token.is_empty() {
            return Ok(());
        }
        request.page_token = page.next_page_token;
    }
}
