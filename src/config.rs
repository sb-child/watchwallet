use alloy::primitives::Address;
use serde::Deserialize;
use url::Url;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// dzserver 配置
    // 约定：不配置时只监听和渲染，不提交打印。
    #[serde(default)]
    pub dzserver: Option<DzserverConfig>,
    /// EVM 链配置
    pub evm: Box<[EvmConfig]>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DzserverConfig {
    /// dzserver gRPC 地址
    pub grpc_url: Url,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EvmConfig {
    /// 区块链名称
    pub chain_name: Box<str>,
    /// 原生币名称
    pub coin: Box<str>,
    /// 区块链 Websocket RPC 地址
    pub ws_url: Url,
    /// 是否监听原生币转账
    // 约定：默认开启。原生币转账没有日志，需要为每个新区块拉取完整区块，
    // 出块快的链（例如 Arbitrum）RPC 消耗会明显变大，可以设为 false。
    #[serde(default = "default_true")]
    pub native_transfer: bool,
    /// 监听地址
    pub address: Box<[EvmAddressConfig]>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
pub struct EvmAddressConfig {
    /// 地址名称
    pub name: Box<str>,
    /// 地址
    pub addr: Address,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(extra: &str) -> Config {
        let text = format!(
            r#"
[[evm]]
chain_name = "Ethereum Mainnet"
ws_url = "wss://example.com/v2/key"
coin = "ETH"
{extra}
[[evm.address]]
name = "wallet"
addr = "0x5838c07a50894fAA58EBfe5E5975f1FCc8792cD3"
"#
        );
        toml::from_str(&text).unwrap()
    }

    #[test]
    fn native_transfer_defaults_to_true() {
        assert!(parse("").evm[0].native_transfer);
    }

    #[test]
    fn native_transfer_can_be_disabled() {
        assert!(!parse("native_transfer = false").evm[0].native_transfer);
    }

    #[test]
    fn dzserver_is_optional() {
        assert!(parse("").dzserver.is_none());
    }

    #[test]
    fn dzserver_parses_grpc_url() {
        let text = r#"
[dzserver]
grpc_url = "http://127.0.0.1:50051"

[[evm]]
chain_name = "Ethereum Mainnet"
ws_url = "wss://example.com/v2/key"
coin = "ETH"
[[evm.address]]
name = "wallet"
addr = "0x5838c07a50894fAA58EBfe5E5975f1FCc8792cD3"
"#;
        let config: Config = toml::from_str(text).unwrap();
        let url = config.dzserver.unwrap().grpc_url;
        assert_eq!(url.scheme(), "http");
        assert_eq!(url.host_str(), Some("127.0.0.1"));
        assert_eq!(url.port(), Some(50051));
    }
}
