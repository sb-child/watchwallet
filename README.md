# WatchWallet

链上钱包活动监听器

- [x] 配置文件和解析
- [x] EVM 链上接口
- [x] dzserver 通知后端
- [ ] 文档

## 介绍

用于在钱包活动时发出通知，比如收款和转账通知。

---

## 链上接口

- [Alloy (EVM)](https://github.com/alloy-rs/alloy)

## 通知后端

- [dzserver](https://github.com/sb-child/dz-print#%E6%89%93%E5%8D%B0%E6%9C%8D%E5%8A%A1%E5%99%A8dzserver)

配置 `[dzserver]` 后，钱包事件会渲染成打印包并提交给 dzserver；不配置时只渲染，不提交打印。

## dzserver 参数示例

```bash
dzserver /tmp/dzserver '127.0.0.1:54321' --font /usr/local/share/fonts/LXGWWenKaiGB/LXGWWenKaiGB-Light.ttf --font /usr/local/share/fonts/LXGWWenKaiGB/LXGWWenKaiGB-Medium.ttf --font /usr/local/share/fonts/LXGWWenKaiGB/LXGWWenKaiGB-Regular.ttf --font /usr/share/fonts/jetbrains-mono-fonts/JetBrainsMono-ExtraBold.otf --font /usr/share/fonts/jetbrains-mono-fonts/JetBrainsMono-Regular.otf
```

准备好相关字体。

## License

MIT
