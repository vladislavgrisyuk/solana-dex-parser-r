use crate::core::constants::dex_program_names;
use base64_simd::STANDARD;

/// Get instruction data bytes from a SolanaInstruction.
/// Decodes base64 string to bytes. Fast path: no caching, no logging, no fallbacks.
#[inline(always)]
pub fn get_instruction_data(instruction: &crate::types::SolanaInstruction) -> Vec<u8> {
    if instruction.data.is_empty() {
        return Vec::new();
    }
    STANDARD.decode_to_vec(&instruction.data).expect("base64 decode failed")
}

/// Get the name of a program by its ID.
/// Returns "Unknown DEX" if not found.
pub fn get_program_name(program_id: &str) -> &'static str {
    dex_program_names::name(program_id)
}

