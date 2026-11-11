#set page(width: 48mm, height: auto, margin: (top: 1mm, bottom: 1mm, left: 0.2mm, right: 0mm))
#set text(font: "LXGW WenKai GB", size: 8pt)
#let wrap(x) = {
  x.codepoints().join(sym.zws)
}
#import "common.typ"

// ---

#common.title("erc20-recv")
#common.display-value(val: 111.333333, sym: "USDT", k: "inc", name: "Tether USD")

#grid(
  gutter: 2pt,
  [#common.account(name: "sbchild钱包1", addr: "0x5838c07a50894fAA58EBfe5E5975f1FCc8792cD3")],
  [#common.flow-up(label: "来自")],
  [#common.account(addr: "0x5838c07a50894fAA58EBfe5E5975f1FCc8792cD3")],
)

#v(-5pt)

#common.details(
  _chain_name: "Arbitrum One",
  _datetime: "2026/06/06 66:66:66",
  _tx_fee: "0.3333333332222333",
  _sym: "ETH",
  _token_contract: "0x55d398326f99059ff775485246999027b3197955",
  _txid: "0xce656824ee4da940ca08901088940d52b4fe1b5a7b4b21007daaec4f1c315529",
)

#line(length: 100%, stroke: (dash: ("dot", 3pt, 3pt, 3pt), cap: "round", thickness: 0.8pt))
