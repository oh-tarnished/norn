//! Splitting a line into fields. RFC 4180: a quoted field may hold commas, quotes
//! (doubled) and newlines.

use std::borrow::Cow;

/// The fields of one record, its line end removed. Borrows `line` when no field
/// is quoted, which is nearly always. `Err` says what is wrong.
pub(crate) fn fields(line: &str) -> Result<Vec<Cow<'_, str>>, &'static str> {
    if !line.contains('"') {
        return Ok(line.split(',').map(Cow::Borrowed).collect());
    }
    let mut out = Vec::with_capacity(8);
    let mut rest = line;
    loop {
        if let Some(quoted) = rest.strip_prefix('"') {
            let (field, after) = quoted_field(quoted)?;
            out.push(Cow::Owned(field));
            match after.strip_prefix(',') {
                Some(next) => rest = next,
                None if after.is_empty() => return Ok(out),
                None => return Err("text after a closing quote"),
            }
        } else {
            let (field, after) = rest.split_once(',').unwrap_or((rest, ""));
            if field.contains('"') {
                return Err("a quote inside an unquoted field");
            }
            out.push(Cow::Borrowed(field));
            if !rest.contains(',') {
                return Ok(out);
            }
            rest = after;
        }
    }
}

/// The field at the start of `text`, just after its opening quote, and what
/// follows its closing quote.
fn quoted_field(text: &str) -> Result<(String, &str), &'static str> {
    let mut field = String::new();
    let mut rest = text;
    loop {
        let Some((part, after)) = rest.split_once('"') else {
            return Err("a quoted field never closes");
        };
        field.push_str(part);
        match after.strip_prefix('"') {
            Some(next) => {
                field.push('"');
                rest = next;
            }
            None => return Ok((field, after)),
        }
    }
}

/// Appends `field`, quoted if it must be.
pub(crate) fn write_field(field: &str, out: &mut String) {
    if field.contains([',', '"', '\n', '\r']) {
        out.push('"');
        out.push_str(&field.replace('"', "\"\""));
        out.push('"');
    } else {
        out.push_str(field);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_field_survives_quoting_whatever_it_holds() {
        for value in [
            "plain",
            "",
            "a,b",
            "say \"hi\"",
            "\"",
            "line\r\nbreak",
            ",\"\n",
        ] {
            let mut line = "x,".to_owned();
            write_field(value, &mut line);
            line.push_str(",y");
            let got = fields(&line).unwrap();
            assert_eq!(got, vec!["x", value, "y"], "{line}");
        }
    }

    #[test]
    fn a_broken_quote_is_an_error_not_a_guess() {
        assert!(fields("a,\"b").is_err());
        assert!(fields("a,\"b\"c,d").is_err());
        assert!(fields("a,b\"c").is_err());
    }
}
