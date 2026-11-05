//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The iceoryx2 half of the plane: rings, their producers, and the node both
//! ends configure alike. Unix only: a Windows daemon never offers a ring
//! (`platform/windows.rs`), so iceoryx2 is not built there.

mod attach;
mod node;
mod producer;
mod ring;

pub use attach::SharedMemory;
pub use producer::Producer;
pub use ring::Ring;
