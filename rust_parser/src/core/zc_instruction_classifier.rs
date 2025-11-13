//! Zero-copy instruction classifier for ZcAdapter
//! 
//! This classifier works directly with ZcInstruction and ZcAdapter,
//! avoiding any allocations or String conversions.
//!
//! System programs and skip programs are compared directly as 32-byte arrays,
//! without base58 encoding/decoding during classification.

use std::collections::{HashMap, HashSet};
use once_cell::sync::Lazy;

use crate::core::utils::get_instruction_data_zc;
use crate::core::zc_adapter::ZcAdapter;
use crate::core::zero_copy::ZcInstruction;
use bs58;

/// System programs as 32-byte arrays (decoded once at startup)
static SYSTEM_PROGRAMS_BYTES: Lazy<HashSet<[u8; 32]>> = Lazy::new(|| {
    use crate::core::constants::SYSTEM_PROGRAMS;
    let mut set = HashSet::with_capacity(SYSTEM_PROGRAMS.len());
    for &pid_str in SYSTEM_PROGRAMS {
        if let Ok(decoded) = bs58::decode(pid_str).into_vec() {
            if decoded.len() == 32 {
                let mut key = [0u8; 32];
                key.copy_from_slice(&decoded);
                set.insert(key);
            }
        }
    }
    set
});

/// Skip programs as 32-byte arrays (decoded once at startup)
static SKIP_PROGRAM_IDS_BYTES: Lazy<HashSet<[u8; 32]>> = Lazy::new(|| {
    use crate::core::constants::SKIP_PROGRAM_IDS;
    let mut set = HashSet::with_capacity(SKIP_PROGRAM_IDS.len());
    for &pid_str in SKIP_PROGRAM_IDS {
        if let Ok(decoded) = bs58::decode(pid_str).into_vec() {
            if decoded.len() == 32 {
                let mut key = [0u8; 32];
                key.copy_from_slice(&decoded);
                set.insert(key);
            }
        }
    }
    set
});

/// Zero-copy classified instruction that references original buffer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZcClassifiedInstruction<'a> {
    /// Program ID as 32-byte array (zero-copy: references buffer)
    pub program_id: &'a [u8; 32],
    /// Outer instruction index
    pub outer_index: usize,
    /// Inner instruction index (None for outer instructions)
    pub inner_index: Option<usize>,
    /// Zero-copy instruction (references buffer)
    pub instruction: &'a ZcInstruction<'a>,
}

/// Zero-copy instruction classifier
/// 
/// Groups instructions by program ID without allocating String copies.
/// Uses program ID as 32-byte arrays for comparison.
pub struct ZcInstructionClassifier<'a> {
    /// Instructions grouped by program ID (32-byte array key)
    instruction_map: HashMap<[u8; 32], Vec<ZcClassifiedInstruction<'a>>>,
    /// Order of first appearance (for deterministic output)
    order: Vec<[u8; 32]>,
}

impl<'a> ZcInstructionClassifier<'a> {
    /// Create new zero-copy instruction classifier
    /// 
    /// # Arguments
    /// * `adapter` - Zero-copy adapter with transaction data
    /// 
    /// # Returns
    /// Classifier that groups instructions by program ID
    pub fn new(adapter: &'a ZcAdapter<'a>) -> Self {
        #[cfg(debug_assertions)]
        let t0 = std::time::Instant::now();
        
        // Pre-allocate with estimated capacity
        let outer_count = adapter.instructions().len();
        let mut instruction_map: HashMap<[u8; 32], Vec<ZcClassifiedInstruction<'a>>> = 
            HashMap::with_capacity(outer_count / 2);
        let mut order: Vec<[u8; 32]> = Vec::with_capacity(outer_count / 2);
        let mut seen: std::collections::HashSet<[u8; 32]> = 
            std::collections::HashSet::with_capacity(outer_count / 2);

        // OUTER instructions - ZERO-COPY: uses 32-byte array keys
        for (outer_index, instruction) in adapter.instructions().iter().enumerate() {
            // Get program ID (zero-copy: reference to buffer)
            let program_id = match adapter.program_id(instruction) {
                Some(pid) => pid,
                None => continue,
            };
            
            // Skip system programs (zero-copy: compare 32-byte arrays directly)
            if SYSTEM_PROGRAMS_BYTES.contains(program_id) {
                continue;
            }
            if SKIP_PROGRAM_IDS_BYTES.contains(program_id) {
                continue;
            }
            
            let classified = ZcClassifiedInstruction {
                program_id,
                outer_index,
                inner_index: None,
                instruction,
            };
            
            instruction_map
                .entry(*program_id)
                .or_default()
                .push(classified);
            
            if seen.insert(*program_id) {
                order.push(*program_id);
            }
        }
        
        #[cfg(debug_assertions)]
        let t1 = std::time::Instant::now();

        // INNER instructions - ZERO-COPY: parse from JSON on demand
        // NOTE: Inner instructions are in meta JSON, not in the message buffer
        // For now, we skip inner instructions in zero-copy classifier
        // They can be processed separately if needed
        // TODO: Add support for inner instructions from meta JSON
        
        #[cfg(debug_assertions)]
        {
            let t2 = std::time::Instant::now();
            tracing::debug!(
                "ZcInstructionClassifier: processed {} outer instructions",
                adapter.instructions().len()
            );
            tracing::debug!(
                "⏱️  ZcInstructionClassifier::new: outer={:.3}μs ({}), total={:.3}μs",
                (t1 - t0).as_secs_f64() * 1_000_000.0,
                adapter.instructions().len(),
                (t2 - t0).as_secs_f64() * 1_000_000.0,
            );
            tracing::info!(
                "ZcInstructionClassifier: found {} unique program IDs",
                order.len()
            );
        }

        Self {
            instruction_map,
            order,
        }
    }
    
    /// Get all program IDs as iterator (zero-copy: references to 32-byte arrays)
    /// Filters out system programs and skip programs
    pub fn get_all_program_ids_iter(&self) -> impl Iterator<Item = &[u8; 32]> {
        self.order.iter()
    }
    
    /// Get all program IDs as Vec of 32-byte arrays (for compatibility)
    /// NOTE: This creates owned copies of 32-byte arrays
    pub fn get_all_program_ids(&self) -> Vec<[u8; 32]> {
        self.order.clone()
    }
    
    /// Get instructions by program ID (zero-copy: returns references)
    /// 
    /// # Arguments
    /// * `program_id` - Program ID as 32-byte array
    /// 
    /// # Returns
    /// Slice of classified instructions for the program
    pub fn get_instructions(&self, program_id: &[u8; 32]) -> &[ZcClassifiedInstruction<'a>] {
        self.instruction_map
            .get(program_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
    
    /// Get instructions by program ID as String (for compatibility)
    /// 
    /// # Arguments
    /// * `program_id_str` - Program ID as base58 string
    /// 
    /// # Returns
    /// Slice of classified instructions for the program
    /// 
    /// # Note
    /// This decodes base58 string to 32-byte array, which has some overhead
    /// Prefer using `get_instructions` with 32-byte array for maximum performance
    pub fn get_instructions_by_string(&self, program_id_str: &str) -> &[ZcClassifiedInstruction<'a>] {
        // Decode base58 string to 32-byte array
        if let Ok(decoded) = bs58::decode(program_id_str).into_vec() {
            if decoded.len() == 32 {
                let mut key = [0u8; 32];
                key.copy_from_slice(&decoded);
                return self.get_instructions(&key);
            }
        }
        &[]
    }
    
    /// Get instructions by program ID string (convenience method)
    /// Returns iterator over program IDs as base58 strings
    /// NOTE: This allocates strings, use get_all_program_ids_iter() for zero-copy
    pub fn get_all_program_ids_strings(&self) -> Vec<String> {
        self.order.iter()
            .map(|pid| bs58::encode(pid).into_string())
            .collect()
    }
    
    /// Find instruction by discriminator (first `slice` bytes)
    /// 
    /// # Arguments
    /// * `discriminator` - Discriminator bytes to match
    /// * `slice` - Number of bytes to compare
    /// 
    /// # Returns
    /// First matching classified instruction
    pub fn get_instruction_by_discriminator(
        &self,
        discriminator: &[u8],
        slice: usize,
    ) -> Option<ZcClassifiedInstruction<'a>> {
        for instructions in self.instruction_map.values() {
            for ci in instructions {
                // Get instruction data (zero-copy: reference to buffer)
                let data = get_instruction_data_zc(ci.instruction);
                if data.len() >= slice && &data[..slice] == discriminator {
                    return Some(*ci);
                }
            }
        }
        None
    }
    
    /// Flatten all instructions into a single Vec
    /// NOTE: This creates owned copies of references
    pub fn flatten(&self) -> Vec<ZcClassifiedInstruction<'a>> {
        self.instruction_map.values().flatten().copied().collect()
    }
}


