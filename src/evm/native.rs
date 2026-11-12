use super::{
    EventTx, EvmCtx, EvmProvider, emit,
    event::{EvmEvent, NativeTransferEvent, TransferDirection, TxInfo, sides},
};
use alloy::{
    consensus::Transaction as _,
    network::TransactionResponse as _,
    primitives::{Address, B256, U256},
    providers::Provider as _,
    rpc::types::{Block, Transaction},
};
use tokio_util::sync::CancellationToken;

/// 已经确认执行成功的原生币转账，两个通知方向都从它派生。
#[derive(Debug, Clone, Copy)]
struct Transfer {
    from: Address,
    to: Address,
    amount: U256,
    tx_hash: B256,
}

// 约定：只覆盖顶层交易（EOA -> 地址 / EOA -> 合约）。合约内部转账（contract -> 钱包）
// 不在交易列表里，需要 trace 系列接口，暂时不支持。
/// 每个区块一个任务，扫描其中的原生币转账。
pub fn spawn_block(
    provider: EvmProvider,
    block_hash: B256,
    ctx: EvmCtx,
    ev: EventTx,
    ct: CancellationToken,
) {
    tokio::spawn(async move {
        if ct.is_cancelled() {
            return;
        }
        let Some(block) = fetch_block(&provider, block_hash).await else {
            return;
        };
        on_block(&block, &ctx, &ev).await;
    });
}

// 负载均衡后的节点可能还没同步到刚出的区块，查到空结果时短暂重试几次。
/// 查询完整区块。
async fn fetch_block(p: &EvmProvider, block_hash: B256) -> Option<Block> {
    const ATTEMPTS: u32 = 3;
    for attempt in 0..ATTEMPTS {
        match p.get_block_by_hash(block_hash).full().await {
            Ok(Some(block)) => return Some(block),
            Ok(None) => tracing::debug!(%block_hash, attempt, "区块还不可用"),
            Err(err) => tracing::error!(%block_hash, attempt, "查询区块失败: {err}"),
        }
        if attempt + 1 < ATTEMPTS {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
    }
    tracing::warn!(%block_hash, "查询区块失败，跳过该区块");
    None
}

async fn on_block(block: &Block, ctx: &EvmCtx, ev: &EventTx) {
    let Some(transactions) = block.transactions.as_transactions() else {
        tracing::warn!(number = block.header.number, "区块没有返回完整的交易列表");
        return;
    };
    let timestamp = block.header.timestamp;
    tracing::trace!(
        number = block.header.number,
        txs = transactions.len(),
        "扫描区块"
    );
    for tx in transactions {
        on_transaction(tx, timestamp, ctx, ev).await;
    }
}

async fn on_transaction(tx: &Transaction, timestamp: u64, ctx: &EvmCtx, ev: &EventTx) {
    let amount = tx.value();
    if amount.is_zero() {
        return;
    }
    // 合约创建：接收方地址要查回执才知道，先跳过
    let Some(to) = tx.kind().into_to() else {
        return;
    };
    let from = tx.from();
    let from_watched = ctx.is_watched(from);
    let to_watched = ctx.is_watched(to);
    if !from_watched && !to_watched {
        return;
    }
    let tx_hash = tx.tx_hash();

    // 交易列表里的 value 不代表真的转账：执行失败的交易也带 value，要用回执确认
    let Some(tx_info) = ctx.tx_at(tx_hash, timestamp).await else {
        return;
    };
    if !tx_info.success {
        tracing::debug!(%tx_hash, "跳过执行失败的原生币转账");
        return;
    }
    let transfer = Transfer {
        from,
        to,
        amount,
        tx_hash,
    };

    // 被监听地址在两边时发两条，和 ERC-20 的处理保持一致
    if to_watched {
        emit_transfer(transfer, TransferDirection::Recv, &tx_info, ctx, ev).await;
    }
    if from_watched {
        emit_transfer(transfer, TransferDirection::Send, &tx_info, ctx, ev).await;
    }
}

async fn emit_transfer(
    transfer: Transfer,
    direction: TransferDirection,
    tx_info: &TxInfo,
    ctx: &EvmCtx,
    ev: &EventTx,
) {
    let (watched_addr, counterparty_addr) = sides(direction, transfer.from, transfer.to);
    let event = NativeTransferEvent {
        watched: ctx.account(watched_addr).await,
        counterparty: ctx.account(counterparty_addr).await,
        direction,
        amount: transfer.amount,
        tx_hash: transfer.tx_hash,
        tx_info: Some(tx_info.clone()),
    };
    emit(ev, EvmEvent::NativeTransfer(Box::new(event))).await;
}
