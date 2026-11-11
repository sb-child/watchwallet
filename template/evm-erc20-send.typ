#set page(width: 48mm, height: auto, margin: (top: 1mm, bottom: 1mm, left: 0.2mm, right: 0mm))
#set text(font: "LXGW WenKai GB", size: 8pt)
#let wrap(x) = {
  x.codepoints().join(sym.zws)
}
#import "common.typ"

// ---

= #box(image("send.svg", width: 14pt), baseline: 3pt) 转出代币

#align(center, common.fit-text("-111.333333 USDT", max-size: 12pt))

#v(-4pt)

#grid(
  gutter: 2pt,
  [#common.account(name: "sbchild钱包1", addr: "0x5838c07a50894fAA58EBfe5E5975f1FCc8792cD3")],
  [#common.flow-down(label: "ERC-20")],
  [#common.account(addr: "0x5838c07a50894fAA58EBfe5E5975f1FCc8792cD3")],
)

#v(-5pt)

- 时间: #text("2026/06/06 66:66:66", weight: "bold")
- 区块链: #text("Arbitrum One", weight: "bold")
- 网络费: #text("0.0000000011 ETH", weight: "bold")
- 合约:
#v(-16pt) // 把"合约:"与"0x"叠在一起
#common.tx(t: "0x55d398326f99059ff775485246999027b3197955")
#v(-5pt)
- 交易:
#v(-16pt) // 把"交易:"与"0x"叠在一起
#common.tx(t: "0xce656824ee4da940ca08901088940d52b4fe1b5a7b4b21007daaec4f1c315529")

#v(-4pt)

#line(length: 100%, stroke: (dash: ("dot", 3pt, 3pt, 3pt), cap: "round", thickness: 0.8pt))
