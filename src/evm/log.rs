use super::{
    EventTx, EvmCtx,
    event::{EvmEvent, TokenApprovalEvent, TokenTransferEvent, TransferDirection},
};
use alloy::{
    primitives::{Address, B256, Keccak256, U256},
    rpc::types::Log,
};
use std::sync::LazyLock;
use tokio_util::sync::CancellationToken;

/// ERC-20 / ERC-721 Transfer
pub static TRANSFER_EVENT_SIGNATURE: LazyLock<B256> = LazyLock::new(|| {
    let mut h = Keccak256::new();
    // ERC-20
    // T0 hash  T1 from T2 to   Data amount
    // Transfer(address,address,uint256)

    // ERC-721
    // T0 hash  T1 from T2 to   T3 tokenId  Data 0x  amount=1
    // Transfer(address,address,uint256)
    h.update("Transfer(address,address,uint256)");
    h.finalize()
});

/// ERC-20 Approval
pub static APPROVAL_EVENT_SIGNATURE: LazyLock<B256> = LazyLock::new(|| {
    let mut h = Keccak256::new();
    // ERC-20
    // T0 hash  T1 owner  T2 spender  Data amount
    // Approval(address,address,uint256)
    h.update("Approval(address,address,uint256)");
    h.finalize()
});

/// 解析后的 Transfer / Approval 日志：主题固定为两个地址，数据只有一个数量。
struct ParsedLog {
    token: Address,
    topic1: Address,
    topic2: Address,
    amount: U256,
    tx_hash: B256,
}

/// 只接受“3 个主题 + 至少 32 字节数据”的日志，ERC-721 的 4 主题会被过滤掉。
fn parse_log(log: &Log) -> Option<ParsedLog> {
    // 重组的日志要撤销，不能当成新的转账
    if log.removed {
        return None;
    }
    let topics = log.topics();
    if topics.len() != 3 {
        return None;
    }
    let data: &[u8] = &log.data().data;
    Some(ParsedLog {
        token: log.address(),
        topic1: Address::from_word(topics[1]),
        topic2: Address::from_word(topics[2]),
        amount: U256::from_be_slice(data.get(..32)?),
        tx_hash: log.transaction_hash?,
    })
}

/// 按方向决定哪一边是被监听地址，返回 `(watched, counterparty)`。
fn sides(direction: TransferDirection, from: Address, to: Address) -> (Address, Address) {
    match direction {
        TransferDirection::Send => (from, to),
        TransferDirection::Recv => (to, from),
    }
}

/// 处理一条订阅到的日志。
pub async fn on_log(log: Log, direction: TransferDirection, ctx: &EvmCtx, ev: &EventTx) {
    let Some(topic0) = log.topic0().copied() else {
        return;
    };
    if topic0 == *TRANSFER_EVENT_SIGNATURE {
        on_transfer(log, direction, ctx, ev).await;
    } else if topic0 == *APPROVAL_EVENT_SIGNATURE {
        on_approval(log, ctx, ev).await;
    }
}

/// 每条日志单独一个任务，互不阻塞。
///
/// 不在 `tokio::select!` 里取消 handler：moka 的初始化 future 被 abort 会让同 key 的等待者重试，
/// 关机时可能多发一次 RPC。这里只在任务开头检查一次取消状态。
pub fn spawn_log(
    log: Log,
    direction: TransferDirection,
    ctx: EvmCtx,
    ev: EventTx,
    ct: CancellationToken,
) {
    tokio::spawn(async move {
        if ct.is_cancelled() {
            return;
        }
        on_log(log, direction, &ctx, &ev).await;
    });
}

async fn on_transfer(log: Log, direction: TransferDirection, ctx: &EvmCtx, ev: &EventTx) {
    let Some(parsed) = parse_log(&log) else {
        return;
    };
    let Some(event) = build_transfer(parsed, direction, ctx).await else {
        return;
    };
    emit(ev, EvmEvent::TokenTransfer(Box::new(event))).await;
}

async fn on_approval(log: Log, ctx: &EvmCtx, ev: &EventTx) {
    let Some(parsed) = parse_log(&log) else {
        return;
    };
    let Some(event) = build_approval(parsed, ctx).await else {
        return;
    };
    emit(ev, EvmEvent::TokenApproval(Box::new(event))).await;
}

async fn build_transfer(
    parsed: ParsedLog,
    direction: TransferDirection,
    ctx: &EvmCtx,
) -> Option<TokenTransferEvent> {
    let (watched_addr, counterparty_addr) = sides(direction, parsed.topic1, parsed.topic2);
    let Some(token) = ctx.erc20(parsed.token).await else {
        tracing::debug!(contract = %parsed.token, "跳过非 ERC-20 的 Transfer 日志");
        return None;
    };
    let tx_info = ctx.tx(parsed.tx_hash).await;
    Some(TokenTransferEvent {
        watched: ctx.account(watched_addr).await,
        counterparty: ctx.account(counterparty_addr).await,
        direction,
        token,
        amount: parsed.amount,
        tx_hash: parsed.tx_hash,
        tx_info,
    })
}

async fn build_approval(parsed: ParsedLog, ctx: &EvmCtx) -> Option<TokenApprovalEvent> {
    // T1 owner, T2 spender
    let Some(token) = ctx.erc20(parsed.token).await else {
        tracing::debug!(contract = %parsed.token, "跳过非 ERC-20 的 Approval 日志");
        return None;
    };
    let tx_info = ctx.tx(parsed.tx_hash).await;
    Some(TokenApprovalEvent {
        owner: ctx.account(parsed.topic1).await,
        spender: ctx.account(parsed.topic2).await,
        token,
        amount: parsed.amount,
        tx_hash: parsed.tx_hash,
        tx_info,
    })
}

async fn emit(ev: &EventTx, event: EvmEvent) {
    if ev.send(event).await.is_err() {
        tracing::warn!("发送 EVM 事件失败：接收端已关闭");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::{Bytes, Log as RawLog, U256, address, b256};

    const TOKEN: Address = address!("0x55d398326f99059ff775485246999027b3197955");
    const FROM: Address = address!("0x5838c07a50894fAA58EBfe5E5975f1FCc8792cD3");
    const TO: Address = address!("0x233b7f746aBAc0797E8dBC344b638F1C4225F264");
    const TX: B256 = b256!("0xce656824ee4da940ca08901088940d52b4fe1b5a7b4b21007daaec4f1c315529");

    fn log_with(topics: Vec<B256>, data: &[u8]) -> Log {
        Log {
            inner: RawLog::new(TOKEN, topics, Bytes::copy_from_slice(data)).unwrap(),
            block_hash: None,
            block_number: None,
            block_timestamp: None,
            transaction_hash: Some(TX),
            transaction_index: None,
            log_index: None,
            removed: false,
        }
    }

    fn transfer_log() -> Log {
        let data = U256::from(1234u64).to_be_bytes::<32>();
        log_with(
            vec![*TRANSFER_EVENT_SIGNATURE, FROM.into_word(), TO.into_word()],
            &data,
        )
    }

    #[test]
    fn parse_erc20_transfer() {
        let parsed = parse_log(&transfer_log()).unwrap();
        assert_eq!(parsed.token, TOKEN);
        assert_eq!(parsed.topic1, FROM);
        assert_eq!(parsed.topic2, TO);
        assert_eq!(parsed.amount, U256::from(1234u64));
        assert_eq!(parsed.tx_hash, TX);
    }

    #[test]
    fn reject_erc721_and_short_data() {
        let topics_4 = vec![
            *TRANSFER_EVENT_SIGNATURE,
            FROM.into_word(),
            TO.into_word(),
            B256::ZERO,
        ];
        assert!(parse_log(&log_with(topics_4, &[])).is_none());
        let topics_3 = vec![*TRANSFER_EVENT_SIGNATURE, FROM.into_word(), TO.into_word()];
        assert!(parse_log(&log_with(topics_3, &[0u8; 31])).is_none());
    }

    #[test]
    fn reject_removed_log() {
        let mut log = transfer_log();
        log.removed = true;
        assert!(parse_log(&log).is_none());
    }

    #[test]
    fn sides_follow_direction() {
        assert_eq!(sides(TransferDirection::Send, FROM, TO), (FROM, TO));
        assert_eq!(sides(TransferDirection::Recv, FROM, TO), (TO, FROM));
    }
}
