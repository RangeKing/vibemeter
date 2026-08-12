# 发布 VibeMeter

GitHub Release 以 `apps/desktop/src-tauri/tauri.conf.json` 中的版本号为准。推送匹配的 `v*` 标签后，`Release` workflow 会先执行完整校验，再分别构建 Apple Silicon 与 Intel 的 DMG、ZIP，并上传到同名 GitHub Release。

## 正常发版

1. 同步修改以下四个版本号，并提交：
   - 根目录 `package.json`
   - `apps/desktop/package.json`
   - `apps/desktop/src-tauri/Cargo.toml`
   - `apps/desktop/src-tauri/tauri.conf.json`
2. 在本地执行 `npm run ci` 和 `npm run build`。
3. 创建并推送与应用版本一致的标签，例如：

   ```sh
   git tag -a v0.3.0 -m "VibeMeter v0.3.0"
   git push origin v0.3.0
   ```

4. 在 GitHub Actions 的 `Release` 任务完成后，检查生成的 Release、自动发布说明和下载资产。

也可以在 GitHub Actions 中手动运行 `Release`，并输入同样格式的标签。若标签和 `tauri.conf.json` 版本不一致，任务会在构建前失败。

## 签名边界

当前自动构建使用 Tauri 的 ad-hoc 签名（`signingIdentity: "-"`），不需要仓库 Secret，适合现阶段公开下载和测试。它不是 Apple Developer ID 签名，也没有公证；用户仍可能需要在 macOS“隐私与安全性”中允许首次启动。

准备正式分发时，应改为 Developer ID Application 签名与 Apple notarization，并在仓库中配置对应证书和 Apple 凭据。不要把证书、密码或 API 私钥提交到 Git。
