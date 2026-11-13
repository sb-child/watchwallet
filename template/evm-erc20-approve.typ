#set page(width: 48mm, height: auto, margin: (top: 1mm, bottom: 1mm, left: 0.2mm, right: 0mm))
#set text(font: "LXGW WenKai GB", size: 8pt)
#import "common.typ"
// ---
#let _transfer_value = common.temp(x: "{{transfer_value}}", def: "1111111222.111233")
#let _coin_symbol = common.temp(x: "{{coin_symbol}}", def: "USDT")
#let _coin_name = common.temp(x: "{{coin_name}}", def: "Tether USD")
#let _chain_name = common.temp(x: "{{chain_name}}", def: "Arbitrum One")
#let _datetime = common.temp(x: "{{datetime}}", def: "2026/06/06 66:66:66")
#let _tx_fee = common.temp(x: "{{tx_fee}}", def: "0.3333333332222333")
#let _tx_fee_coin_symbol = common.temp(x: "{{tx_fee_coin_symbol}}", def: "ETH")
#let _tx_id = common.temp(x: "{{tx_id}}", def: "0xce656824ee4da940ca08901088940d52b4fe1b5a7b4b21007daaec4f1c315529")
#let _token_contract = common.temp(x: "{{token_contract}}", def: "0x55d398326f99059ff775485246999027b3197955")
// 持有币的一方
#let _owner_addr = common.temp(x: "{{owner_addr}}", def: "0x5838c07a50894fAA58EBfe5E5975f1FCc8792cD3")
#let _owner_name = common.temp(x: "{{owner_name}}", def: "sbchild钱包1")
#let _owner_is_contract = common.temp(x: "{{owner_is_contract}}", def: false)
// 被授权的一方
#let _spender_addr = common.temp(x: "{{spender_addr}}", def: "0x233b7f746aBAc0797E8dBC344b638F1C4225F264")
#let _spender_name = common.temp(x: "{{spender_name}}", def: "")
#let _spender_is_contract = common.temp(x: "{{spender_is_contract}}", def: true)
// ---

// 授权数量为 0 表示取消授权。
#common.title("approve", revoked: _transfer_value == "0")
// #common.display-value(val: "inf", sym: "USDT", k: "", name: "Tether USD")
#common.display-value(val: _transfer_value, sym: _coin_symbol, k: "", name: _coin_name)
#grid(
  gutter: 2pt,
  [#common.account(addr: _owner_addr, contract: _owner_is_contract, name: _owner_name)],
  [#common.flow-down(label: "授权给")],
  [#common.account(addr: _spender_addr, contract: _spender_is_contract, name: _spender_name)],
)
#v(-5pt)
#common.details(
  _chain_name: _chain_name,
  _datetime: _datetime,
  _tx_fee: _tx_fee,
  _sym: _tx_fee_coin_symbol,
  _token_contract: _token_contract,
  _txid: _tx_id,
)
#line(length: 100%, stroke: (dash: ("dot", 3pt, 3pt, 3pt), cap: "round", thickness: 0.8pt))
