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
    let token = ctx.erc20(parsed.token).await?;
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
    let token = ctx.erc20(parsed.token).await?;
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
