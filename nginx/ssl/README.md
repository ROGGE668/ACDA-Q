# SSL 证书配置

## 开发环境（自签名证书）

```bash
# 生成自签名证书（有效期 365 天）
openssl req -x509 -nodes -days 365 \
  -newkey rsa:2048 \
  -keyout server.key \
  -out server.crt \
  -subj "/CN=localhost"
```

## 生产环境（Let's Encrypt）

```bash
# 安装 certbot
sudo apt install certbot python3-certbot-nginx

# 自动获取并配置证书
sudo certbot --nginx -d your-domain.com

# 证书自动续期
sudo certbot renew --dry-run
```

## 文件放置

将证书文件放在本目录下：
- `server.crt` — 证书文件
- `server.key` — 私钥文件

然后在 `nginx.conf` 中取消 SSL 相关注释：
```nginx
listen 443 ssl;
ssl_certificate /etc/nginx/ssl/server.crt;
ssl_certificate_key /etc/nginx/ssl/server.key;
```
