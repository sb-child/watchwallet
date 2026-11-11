#set page(width: 48mm, height: auto, margin: (top: 1mm, bottom: 1mm, left: 0.2mm, right: 0mm))
#set text(font: "LXGW WenKai GB", size: 8pt)
#import "common.typ"
// ---
#let _transfer_value = common.temp(x: "{{transfer_value}}", def: "111.33333333333")
#let _coin_symbol = common.temp(x: "{{coin_symbol}}", def: "ETH")
#let _coin_name = common.temp(x: "{{coin_name}}", def: "")
#let _chain_name = common.temp(x: "{{chain_name}}", def: "Arbitrum One")
#let _datetime = common.temp(x: "{{datetime}}", def: "2026/06/06 66:66:66")
#let _tx_fee = common.temp(x: "{{tx_fee}}", def: "0.3333333332222333")
#let _tx_fee_coin_symbol = common.temp(x: "{{tx_fee_coin_symbol}}", def: "ETH")
#let _tx_id = common.temp(x: "{{tx_id}}", def: "0xce656824ee4da940ca08901088940d52b4fe1b5a7b4b21007daaec4f1c315529")
// 失去币的一方
#let _sender_addr = common.temp(x: "{{sender_addr}}", def: "0x233b7f746aBAc0797E8dBC344b638F1C4225F264")
#let _sender_name = common.temp(x: "{{sender_name}}", def: "")
#let _sender_is_contract = common.temp(x: "{{sender_is_contract}}", def: false)
// 获得币的一方
#let _recp_addr = common.temp(x: "{{recp_addr}}", def: "0x5838c07a50894fAA58EBfe5E5975f1FCc8792cD3")
#let _recp_name = common.temp(x: "{{recp_name}}", def: "sbchild钱包1")
#let _recp_is_contract = common.temp(x: "{{recp_is_contract}}", def: false)
// ---
#common.title("recv")
#common.display-value(val: _transfer_value, sym: _coin_symbol, k: "inc", name: _coin_name)
#grid(
  gutter: 2pt,
  [#common.account(addr: _recp_addr, contract: _recp_is_contract, name: _recp_name)],
  [#common.flow-up(label: "来自")],
  [#common.account(addr: _sender_addr, contract: _sender_is_contract, name: _sender_name)],
)
#v(-5pt)
#common.details(
  _chain_name: _chain_name,
  _datetime: _datetime,
  _tx_fee: _tx_fee,
  _sym: _tx_fee_coin_symbol,
  _txid: _tx_id,
)
#line(length: 100%, stroke: (dash: ("dot", 3pt, 3pt, 3pt), cap: "round", thickness: 0.8pt))
