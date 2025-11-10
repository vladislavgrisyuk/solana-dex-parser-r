use anyhow::{Context, Result};
use solana_dex_parser::{rpc, DexParser, ParseConfig};

fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
    .with_target(true)
    .with_thread_ids(false)
    .with_level(true)
    .compact()
    .with_max_level(tracing::Level::DEBUG)
    .init();
    
    // Получаем аргументы командной строки
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() < 2 {
        eprintln!("Использование: cargo run --bin parse_tx <signature> [rpc_url]");
        eprintln!("Пример: cargo run --bin parse_tx 5VERv8NMvzbJMEkV8xnrLkEaWRtSz9CosKDYjCJjBRnb");
        std::process::exit(1);
    }

    let signature = &args[1];
    let rpc_url: String = args.get(2)
        .cloned()
        .or_else(|| std::env::var("SOLANA_RPC_URL").ok())
        .unwrap_or_else(|| "https://api.mainnet-beta.solana.com".to_string());

    println!("🔍 Получаю транзакцию {} через RPC {}...", signature, rpc_url);

    // Получаем транзакцию через RPC
    let tx = rpc::fetch_transaction(&rpc_url, signature)
        .with_context(|| format!("Не удалось получить транзакцию {}", signature))?;

    println!("✅ Транзакция получена!");
    println!("   Slot: {}", tx.slot);
    println!("   Signature: {}", tx.signature);
    println!("   Block time: {}", tx.block_time);
    println!("   Signers: {:?}", tx.signers);
    println!("   Instructions: {}", tx.instructions.len());
    println!();

    // Создаем парсер
    let parser = DexParser::new();
    let config = ParseConfig::default();

    println!("📊 Парсинг транзакции...");
    println!();

    // ПЕРВЫЙ ВЫЗОВ - холодный старт (без кэша)
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("🔥 ПЕРВЫЙ ВЫЗОВ (холодный старт, создание кэша)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    
    let first_start = std::time::Instant::now();
    let _result1 = parser.parse_all(tx.clone(), Some(config.clone()));
    let first_duration = first_start.elapsed();
    
    println!("⏱️  ВРЕМЯ ПЕРВОГО ПАРСИНГА:");
    println!("   Общее время: {:.3}ms ({:.6}s)", 
        first_duration.as_secs_f64() * 1000.0,
        first_duration.as_secs_f64()
    );
    println!("   Скорость: {:.0} транзакций/сек", 1.0 / first_duration.as_secs_f64());
    println!();

    // ВТОРОЙ ВЫЗОВ - прогретый кэш
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("⚡ ВТОРОЙ ВЫЗОВ (прогретый кэш, все должно быть быстрее)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    
    let second_start = std::time::Instant::now();
    let result = parser.parse_all(tx, Some(config));
    let second_duration = second_start.elapsed();
    
    println!("⏱️  ВРЕМЯ ВТОРОГО ПАРСИНГА:");
    println!("   Общее время: {:.3}ms ({:.6}s)", 
        second_duration.as_secs_f64() * 1000.0,
        second_duration.as_secs_f64()
    );
    println!("   Скорость: {:.0} транзакций/сек", 1.0 / second_duration.as_secs_f64());
    println!();
    
    let speedup = first_duration.as_secs_f64() / second_duration.as_secs_f64();
    println!("📊 СРАВНЕНИЕ:");
    println!("   Первый вызов:  {:.3}ms", first_duration.as_secs_f64() * 1000.0);
    println!("   Второй вызов:  {:.3}ms", second_duration.as_secs_f64() * 1000.0);
    println!("   Ускорение:     {:.2}x", speedup);
    println!("   Экономия:      {:.3}ms ({:.1}%)", 
        (first_duration - second_duration).as_secs_f64() * 1000.0,
        ((first_duration - second_duration).as_secs_f64() / first_duration.as_secs_f64()) * 100.0
    );
    println!();

    // Выводим результаты
    println!("═══════════════════════════════════════════════════════════");
    println!("📈 РЕЗУЛЬТАТЫ ПАРСИНГА");
    println!("═══════════════════════════════════════════════════════════");
    println!();

    // Статус транзакции
    println!("Статус: {:?}", result.tx_status);
    println!("Fee: {} SOL", result.fee.ui_amount.unwrap_or(0.0));
    println!("Compute units: {}", result.compute_units);
    println!();

    // Трейды
    if !result.trades.is_empty() {
        println!("🔄 ТРЕЙДЫ ({}):", result.trades.len());
        for (i, trade) in result.trades.iter().enumerate() {
            println!("  [{}/{}] {:?}", i + 1, result.trades.len(), trade.trade_type);
            println!("     Input:  {} {} (raw: {})", 
                trade.input_token.amount, 
                trade.input_token.mint.chars().take(8).collect::<String>(),
                trade.input_token.amount_raw
            );
            println!("     Output: {} {} (raw: {})", 
                trade.output_token.amount,
                trade.output_token.mint.chars().take(8).collect::<String>(),
                trade.output_token.amount_raw
            );
            if let Some(ref amm) = trade.amm {
                println!("     AMM: {}", amm);
            }
            if let Some(ref program_id) = trade.program_id {
                println!("     Program: {}", program_id);
            }
            println!();
        }
    } else {
        println!("🔄 Трейды: не найдено");
        println!();
    }

    // Ликвидность
    if !result.liquidities.is_empty() {
        println!("💧 ЛИКВИДНОСТЬ ({}):", result.liquidities.len());
        for (i, pool) in result.liquidities.iter().enumerate() {
            println!("  [{}/{}] {:?} - Pool: {}", 
                i + 1, 
                result.liquidities.len(), 
                pool.event_type,
                pool.pool_id.chars().take(16).collect::<String>()
            );
            if let Some(ref token0) = pool.token0_mint {
                println!("     Token0: {} (amount: {:?})", 
                    token0.chars().take(8).collect::<String>(),
                    pool.token0_amount
                );
            }
            if let Some(ref token1) = pool.token1_mint {
                println!("     Token1: {} (amount: {:?})", 
                    token1.chars().take(8).collect::<String>(),
                    pool.token1_amount
                );
            }
            println!();
        }
    } else {
        println!("💧 Ликвидность: не найдено");
        println!();
    }

    // Трансферы
    if !result.transfers.is_empty() {
        println!("💸 ТРАНСФЕРЫ ({}):", result.transfers.len());
        for (i, transfer) in result.transfers.iter().enumerate() {
            println!("  [{}/{}] {} -> {}", 
                i + 1,
                result.transfers.len(),
                transfer.info.source.chars().take(8).collect::<String>(),
                transfer.info.destination.chars().take(8).collect::<String>()
            );
            println!("     Mint: {}", transfer.info.mint.chars().take(8).collect::<String>());
            println!("     Amount: {} (raw: {})", 
                transfer.info.token_amount.ui_amount.unwrap_or(0.0),
                transfer.info.token_amount.amount
            );
            println!("     Program: {}", transfer.program_id);
            println!();
        }
    } else {
        println!("💸 Трансферы: не найдено");
        println!();
    }

    // Мем-ивенты
    if !result.meme_events.is_empty() {
        println!("🎯 MEME СОБЫТИЯ ({}):", result.meme_events.len());
        for (i, meme) in result.meme_events.iter().enumerate() {
            println!("  [{}/{}] {:?}", i + 1, result.meme_events.len(), meme.event_type);
            println!("     Base mint: {}", meme.base_mint.chars().take(8).collect::<String>());
            println!("     Quote mint: {}", meme.quote_mint.chars().take(8).collect::<String>());
            if let Some(ref name) = meme.name {
                println!("     Name: {}", name);
            }
            if let Some(ref symbol) = meme.symbol {
                println!("     Symbol: {}", symbol);
            }
            println!();
        }
    } else {
        println!("🎯 Meme события: не найдено");
        println!();
    }

    // Балансы
    if let Some(ref sol_change) = result.sol_balance_change {
        println!("💰 SOL баланс:");
        println!("   Pre:  {} SOL", sol_change.pre as f64 / 1e9);
        println!("   Post: {} SOL", sol_change.post as f64 / 1e9);
        println!("   Change: {} SOL", sol_change.change as f64 / 1e9);
        println!();
    }

    if !result.token_balance_change.is_empty() {
        println!("🪙 TOKEN балансы:");
        for (account, change) in &result.token_balance_change {
            println!("   Account: {}", account.chars().take(16).collect::<String>());
            println!("     Change: {} (raw)", change.change);
        }
        println!();
    }

    // JSON вывод (опционально, если нужен полный вывод)
    if args.contains(&"--json".to_string()) {
        println!("═══════════════════════════════════════════════════════════");
        println!("📄 ПОЛНЫЙ JSON ВЫВОД:");
        println!("═══════════════════════════════════════════════════════════");
        println!("{}", serde_json::to_string_pretty(&result)?);
    }

    Ok(())
}

