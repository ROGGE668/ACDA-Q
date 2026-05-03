# GitHub Actions 自动部署配置指南

## 1. 创建 GitHub 仓库

在 GitHub 上创建一个新仓库（建议命名为 `ACDA-Quant`）。

## 2. 配置 GitHub Secrets

在仓库页面 → Settings → Secrets and variables → Actions → New repository secret，添加以下 secrets：

| Secret Name | Value | 说明 |
|------------|-------|------|
| `SSH_PRIVATE_KEY` | 你的 SSH 私钥完整内容 | 即 `yuanluezixun.pem` 的内容 |
| `SERVER_IP` | `124.220.70.210` | 服务器 IP |
| `SERVER_USER` | `ubuntu` | SSH 用户名 |
| `REMOTE_DIR` | `~/ACDA-Q` | 服务器上项目路径（可选，默认 ~/ACDA-Q）|

### 获取 SSH_PRIVATE_KEY

在本地终端执行：
```bash
cat /Users/hong/Documents/yuanluezixun.pem
```
复制输出的全部内容（包括 `-----BEGIN OPENSSH PRIVATE KEY-----` 和 `-----END OPENSSH PRIVATE KEY-----`），粘贴到 Secret 中。

## 3. 推送代码到 GitHub

```bash
cd /Users/hong/Documents/ACDA-Q

# 添加远程仓库（替换 YOUR_USERNAME 为你的 GitHub 用户名）
git remote add origin https://github.com/YOUR_USERNAME/ACDA-Quant.git

# 或者使用 SSH
git remote add origin git@github.com:YOUR_USERNAME/ACDA-Quant.git

# 添加并提交代码
git add .
git commit -m "Initial commit"

# 推送到 main 分支
git branch -M main
git push -u origin main
```

## 4. 验证自动部署

推送后，GitHub Actions 会自动触发：

1. 访问 GitHub 仓库 → Actions 标签页
2. 查看 deploy workflow 的运行状态
3. 成功后，服务器会自动更新

你也可以手动触发：仓库页面 → Actions → Deploy to Server → Run workflow。

## 5. 部署后的验证

```bash
# 在本地测试 API
 curl http://124.220.70.210:8000/health
```

## 6. 日常开发流程

修改代码后，只需执行：
```bash
git add .
git commit -m "你的修改说明"
git push origin main
```

GitHub Actions 会自动部署到服务器。

## 注意事项

1. **.env 文件不会通过 Git 同步**，需要在服务器上单独创建
2. **首次部署前**，确保服务器上已有 `.env` 文件和数据库
3. 如果部署失败，查看 GitHub Actions 日志定位问题
