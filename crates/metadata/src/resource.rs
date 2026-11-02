//! Client-chosen resource IDs, the last segment of a resource name.

use crate::error::MetadataError;

/// The longest resource ID, per the schema.
const MAX_ID_BYTES: usize = 63;

/// The longest `display_name`, in characters, per the schema.
pub const MAX_DISPLAY_NAME_CHARS: usize = 256;

/// Checks `id` is a legal resource ID: 1 to 63 characters of `[a-z0-9-]`,
/// starting with a letter (AIP-122), so a name is safe in a URL and a log.
/// Borrows `id`; allocates only for the error.
///
/// # Errors
/// [`MetadataError::Invalid`] naming `field`.
pub fn check_resource_id(field: &'static str, id: &str) -> Result<(), MetadataError> {
    let invalid = |reason: &str| {
        Err(MetadataError::Invalid {
            field,
            reason: reason.to_owned(),
        })
    };
    if id.is_empty() || id.len() > MAX_ID_BYTES {
        return invalid("must be 1 to 63 characters");
    }
    if !id.starts_with(|c: char| c.is_ascii_lowercase()) {
        return invalid("must start with a lowercase letter");
    }
    if !id
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return invalid("must contain only [a-z0-9-]");
    }
    Ok(())
}

/// Checks a `display_name` is at most [`MAX_DISPLAY_NAME_CHARS`] characters.
///
/// # Errors
/// [`MetadataError::Invalid`].
pub fn check_display_name(name: &str) -> Result<(), MetadataError> {
    if name.chars().count() > MAX_DISPLAY_NAME_CHARS {
        return Err(MetadataError::Invalid {
            field: "display_name",
            reason: format!("must be at most {MAX_DISPLAY_NAME_CHARS} characters"),
        });
    }
    Ok(())
}
