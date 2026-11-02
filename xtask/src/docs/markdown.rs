//! Just enough Markdown to find links and heading anchors.
//!
//! Not a parser: a hand-written reading of the shapes these documents use,
//! because the gates take no dependency they can avoid. Fenced code blocks and
//! inline code are blanked first, so a link shown as an example is not a link.

/// One link or image in a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// 1-based line number.
    pub line: usize,
    /// The target exactly as written, without a title.
    pub target: String,
}

/// `text`'s lines with fenced code blocks emptied. Line numbers are kept.
#[must_use]
pub fn prose(text: &str) -> Vec<String> {
    let mut fence: Option<char> = None;
    text.lines()
        .map(|line| {
            let bare = line.trim_start_matches([' ', '>']);
            let marker = ['`', '~']
                .into_iter()
                .find(|m| bare.starts_with(&String::from(*m).repeat(3)));
            match (fence, marker) {
                (None, Some(m)) => fence = Some(m),
                (Some(open), Some(m)) if open == m => fence = None,
                _ => {}
            }
            if fence.is_some() || marker.is_some() {
                String::new()
            } else {
                line.to_owned()
            }
        })
        .collect()
}

/// `line` with each inline code span replaced by spaces, so link labels that
/// hold code keep their shape.
#[must_use]
pub fn blank_code(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        if c != '`' {
            out.push(c);
            i += 1;
            continue;
        }
        let run = chars.iter().skip(i).take_while(|ch| **ch == '`').count();
        match closing(&chars, i + run, run) {
            Some(end) => {
                out.extend(std::iter::repeat_n(' ', end + run - i));
                i = end + run;
            }
            None => {
                out.extend(std::iter::repeat_n('`', run));
                i += run;
            }
        }
    }
    out
}

/// Where a run of exactly `run` backticks starts, at or after `from`.
fn closing(chars: &[char], from: usize, run: usize) -> Option<usize> {
    let mut i = from;
    while i < chars.len() {
        let here = chars.iter().skip(i).take_while(|ch| **ch == '`').count();
        if here == run {
            return Some(i);
        }
        i += here.max(1);
    }
    None
}

/// Every inline link and image, reference definition and HTML `src` or `href`.
#[must_use]
pub fn links(text: &str) -> Vec<Link> {
    let mut found = Vec::new();
    for (index, line) in prose(text).iter().enumerate() {
        let line_text = blank_code(line);
        let mut push = |target: String| {
            found.push(Link {
                line: index + 1,
                target,
            });
        };
        inline_targets(&line_text).into_iter().for_each(&mut push);
        if let Some(target) = reference_definition(&line_text) {
            push(target);
        }
        for attribute in ["src=\"", "href=\""] {
            for (at, _) in line_text.match_indices(attribute) {
                let rest = line_text.get(at + attribute.len()..).unwrap_or("");
                if let Some((target, _)) = rest.split_once('"') {
                    push(target.to_owned());
                }
            }
        }
    }
    found
}

fn inline_targets(line: &str) -> Vec<String> {
    let mut targets = Vec::new();
    for (at, _) in line.match_indices("](") {
        let rest = line.get(at + 2..).unwrap_or("");
        let mut depth = 0usize;
        let end = rest.char_indices().find(|(_, c)| {
            match c {
                '(' => depth += 1,
                ')' if depth == 0 => return true,
                ')' => depth -= 1,
                _ => {}
            }
            false
        });
        let Some((end, _)) = end else { continue };
        let inner = rest.get(..end).unwrap_or("").trim();
        let target = inner.split_whitespace().next().unwrap_or("");
        let target = target.trim_start_matches('<').trim_end_matches('>');
        targets.push(target.to_owned());
    }
    targets
}

fn reference_definition(line: &str) -> Option<String> {
    let rest = line.trim_start().strip_prefix('[')?;
    let (label, rest) = rest.split_once("]:")?;
    let target = rest.split_whitespace().next()?;
    (!label.is_empty()).then(|| target.to_owned())
}
