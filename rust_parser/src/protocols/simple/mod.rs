mod simple_liquidity;
mod simple_meme;
mod simple_trade;
mod simple_transfer;

pub use simple_liquidity::SimpleLiquidityParser;
pub use simple_meme::SimpleMemeParser;
pub use simple_trade::SimpleTradeParser;
pub use simple_transfer::SimpleTransferParser;

pub trait TradeParser {
    fn process_trades(&mut self) -> Vec<crate::types::TradeInfo>;
}

pub trait LiquidityParser {
    fn process_liquidity(&mut self) -> Vec<crate::types::PoolEvent>;
}

pub trait TransferParser {
    fn process_transfers(&mut self) -> Vec<crate::types::TransferData>;
}

pub trait MemeEventParser {
    fn process_events(&mut self) -> Vec<crate::types::MemeEvent>;
}
