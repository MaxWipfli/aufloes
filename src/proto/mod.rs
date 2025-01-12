// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2025 Max Wipfli <mail@maxwipfli.ch>

/// Extracts the transaction ID from a message encoded in DNS wire format.
pub fn txid_from_binary_message(message: &[u8]) -> u16 {
    let mut txid = [0u8; 2];
    txid.copy_from_slice(&message[0..2]);
    u16::from_be_bytes(txid)
}
