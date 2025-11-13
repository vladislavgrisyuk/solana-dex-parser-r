use crate::core::constants::TOKENS;
use crate::types::TransferData;

/// Получает LP transfers (токены для ликвидности)
/// Аналог getLPTransfers из TypeScript
#[inline]
pub fn get_lp_transfers(transfers: &[TransferData]) -> Vec<&TransferData> {
    let tokens: Vec<&TransferData> = transfers
        .iter()
        .filter(|t| t.transfer_type.contains("transfer"))
        .collect();

    if tokens.len() >= 2 {
        let first = tokens[0];
        let second = tokens[1];
        
        // Если первый токен - SOL, или первый - supported token, а второй - нет
        if first.info.mint == TOKENS.SOL
            || (is_supported_token(&first.info.mint) && !is_supported_token(&second.info.mint))
        {
            return vec![second, first];
        }
    }
    
    tokens
}

#[inline]
fn is_supported_token(mint: &str) -> bool {
    TOKENS.values().contains(&mint)
}

/// Конвертация raw amount в UI amount
#[inline]
pub fn convert_to_ui_amount(amount: u128, decimals: u8) -> f64 {
    if decimals == 0 {
        return amount as f64;
    }

    const POW10: [f64; 20] = [
        1.0, 10.0, 100.0, 1_000.0, 10_000.0, 100_000.0, 1_000_000.0, 10_000_000.0,
        100_000_000.0, 1_000_000_000.0, 10_000_000_000.0, 100_000_000_000.0,
        1_000_000_000_000.0, 10_000_000_000_000.0, 100_000_000_000_000.0,
        1_000_000_000_000_000.0, 10_000_000_000_000_000.0, 100_000_000_000_000_000.0,
        1_000_000_000_000_000_000.0, 10_000_000_000_000_000_000.0,
    ];

    let d = decimals as usize;
    let scale = if d < POW10.len() {
        POW10[d]
    } else {
        10f64.powi(decimals as i32)
    };

    (amount as f64) / scale
}

