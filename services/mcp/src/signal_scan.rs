//! Walking the selected signals a page at a time, from a cursor. A pull, not a
//! callback, because what a tool does with each signal is itself a series of calls.

use std::collections::VecDeque;

use norn_protocol::signal::{ListSignalsRequest, Signal};

use crate::cursor::{Cursor, Key, Order};
use crate::daemon::Call;
use crate::error::ToolError;
use crate::select::Selection;

/// Where a walk over signals stands.
#[derive(Debug)]
pub(crate) struct SignalScan<'a> {
    call: &'a Call,
    selection: Selection,
    cursor: Cursor,
    page_size: i32,
    /// The token of the page `waiting` came from.
    page: String,
    next: Option<String>,
    waiting: VecDeque<Signal>,
    last: Option<Key>,
}

impl<'a> SignalScan<'a> {
    pub(crate) fn new(
        call: &'a Call,
        selection: Selection,
        cursor: Cursor,
        page_size: usize,
    ) -> Self {
        Self {
            call,
            selection,
            page: cursor.r.clone(),
            next: Some(cursor.r.clone()),
            page_size: i32::try_from(page_size).unwrap_or(i32::MAX),
            waiting: VecDeque::new(),
            last: None,
            cursor,
        }
    }

    async fn fetch(&mut self, token: String) -> Result<(), ToolError> {
        let request = ListSignalsRequest {
            parent: self.selection.parent.clone(),
            filter: self.selection.filter.clone(),
            page_size: self.page_size,
            page_token: token.clone(),
        };
        let mut client = self.call.signals();
        let page = client.list_signals(request);
        let page = self.call.rpc("SignalService.ListSignals", page).await?;
        self.page = token;
        self.next = Some(page.next_page_token).filter(|t| !t.is_empty());
        self.waiting.extend(page.signals);
        Ok(())
    }

    /// The next signal not yet given, without consuming it.
    pub(crate) async fn peek(&mut self) -> Result<Option<&Signal>, ToolError> {
        loop {
            while let Some(signal) = self.waiting.front() {
                if !self.cursor.given(&Key::name(&signal.name), Order::Listing) {
                    return Ok(self.waiting.front());
                }
                self.waiting.pop_front();
            }
            match self.next.take() {
                Some(token) => self.fetch(token).await?,
                None => return Ok(None),
            }
        }
    }

    /// Marks the signal [`Self::peek`] showed as given.
    pub(crate) fn consume(&mut self) {
        if let Some(signal) = self.waiting.pop_front() {
            self.last = Some(Key::name(&signal.name));
        }
    }

    /// The cursor for what has not been consumed, called when the walk stops early.
    pub(crate) fn rest(self) -> Cursor {
        Cursor {
            r: self.page,
            a: self.last.or(self.cursor.a),
            f: self.cursor.f,
            n: self.cursor.n,
            p: None,
        }
    }
}
