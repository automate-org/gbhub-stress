# GBHub-Stress

> GB28181 压力测试工具 — 模拟海量设备注册、心跳和实时播放

## 特性

- 🚀 单机可模拟 **50,000+** 设备同时注册
- 📡 所有设备共享 **ZLM 固定流**，节省资源
- 🔄 自动维护注册与心跳，支持认证
- 📊 支持上级发起 INVITE 播放请求
- 💾 内存存储会话，无需 Redis/数据库
- ⚡ 基于 Tokio 异步运行时，高性能

## 快速开始

### 编译

```bash
cargo build --release
```

运行
```bash
bash run.sh
```
性能参考
场景	模拟上限	瓶颈
注册 + 心跳	~50,000	UDP 端口数
并发播放	~5,000~20,000	ZLM RTP 端口范围
极限调优后	~100,000+	文件描述符 + 内存调优

```bash
# 提高文件描述符限制
ulimit -n 655350
```
## GBHub-Stress 环境变量配置说明

GBHub-Stress 支持的所有环境变量，按功能分类说明。

---

### 1. 必需配置

| 变量名 | 说明 | 示例 |
|--------|------|------|
| `UPSTREAM_IP` | 上级 SIP 服务器 IP 地址 | `192.168.1.100` |
| `FIXED_STREAM` | ZLMediaKit 中预先存在的流名，格式 `app/stream` | `rtp/test_stream` |
| `ZLM_API_BASE` | ZLMediaKit HTTP API 地址 | `http://127.0.0.1:9080` |
| `ZLM_SECRET` | ZLMediaKit API 密钥（与 config.ini 中的 `secret` 一致） | `your_secret_key` |

---

### 2. 设备模拟配置

| 变量名 | 默认值 | 说明 |
|--------|--------|------|
| `DEVICE_COUNT` | `100` | 模拟设备总数 |
| `BASE_PORT` | `15000` | 本地 UDP 起始端口，每个设备递增 1 |
| `DEVICE_ID_PREFIX` | `3402000000` | 设备 ID 前缀，最终 ID 为 20 位数字，如 `34020000000000000001` |
| `PASSWORD` | `123456` | 设备注册密码（用于 Digest 认证） |
| `PUBLIC_IP` | `127.0.0.1` | 本机公网 IP（用于 SDP 中的 `c=` 行，上级需可达） |

---

### 3. 网络与通信

| 变量名 | 默认值 | 说明 |
|--------|--------|------|
| `UPSTREAM_PORT` | `5060` | 上级 SIP 服务器 UDP 端口 |
| `REALM` | 同 `DEVICE_ID_PREFIX` | SIP 域（Realm），通常与设备 ID 前缀相同 |

---

### 4. 心跳与注册

| 变量名 | 默认值 | 说明 |
|--------|--------|------|
| `HEARTBEAT_INTERVAL` | `30` | 心跳发送间隔（秒） |
| `REGISTER_EXPIRES` | `3600` | 注册过期时间（秒），设备会在过期前 1/4 时间重注册 |

---

### 5. 日志与调试

| 变量名 | 默认值 | 说明 |
|--------|--------|------|
| `RUST_LOG` | `info` | 日志级别，可选 `trace`、`debug`、`info`、`warn`、`error` |
| `ZLM_RETRY_DELAY_MS` | `300` | ZLM API 调用失败后的重试延迟（毫秒） |

---

### 6. 高级调优（可选）

| 变量名 | 默认值 | 说明 |
|--------|--------|------|
| `CATALOG_PAGE_DELAY_MS` | `300` | 目录查询（Catalog）分页发送间隔（毫秒） |
| `STREAM_READY_TIMEOUT_SECS` | `5` | 等待 ZLM 流就绪的超时时间（秒） |
| `SIP_INVITE_TIMEOUT_SECS` | `15` | INVITE 请求等待响应的超时时间（秒） |

---

### 7. 示例配置

```bash
# 必需
UPSTREAM_IP=192.168.28.252
FIXED_STREAM=rtp/test_stream
ZLM_API_BASE=http://127.0.0.1:9080
ZLM_SECRET=your_secret_key_here

# 设备模拟
DEVICE_COUNT=5000
BASE_PORT=10001
DEVICE_ID_PREFIX=3402000000
PASSWORD=123456
PUBLIC_IP=192.168.28.23

# 网络
UPSTREAM_PORT=6060

# 心跳与注册
HEARTBEAT_INTERVAL=30
REGISTER_EXPIRES=3600

# 日志
RUST_LOG=info

```

### 8. 系统调优（推荐）

在运行大规模压测前，建议进行以下系统调优：
#### 8.1 提高文件描述符限制

默认限制通常为 1024，远小于大规模压测需求（每个设备需要多个文件描述符）。
```bash

# 临时生效（当前 shell 会话）
ulimit -n 655350

# 永久生效（编辑 /etc/security/limits.conf）
echo "* soft nofile 655350" | sudo tee -a /etc/security/limits.conf
echo "* hard nofile 655350" | sudo tee -a /etc/security/limits.conf
```
#### 8.2 调整 UDP 缓冲区大小

UDP 缓冲区不足会导致丢包，影响压测准确性。

```bash

# 临时生效
sudo sysctl -w net.core.rmem_max=134217728
sudo sysctl -w net.core.wmem_max=134217728

# 永久生效（编辑 /etc/sysctl.conf）
echo "net.core.rmem_max=134217728" | sudo tee -a /etc/sysctl.conf
echo "net.core.wmem_max=134217728" | sudo tee -a /etc/sysctl.conf
sudo sysctl -p
```

#### 8.3 检查当前限制
```bash

# 查看当前文件描述符限制
ulimit -n

# 查看当前 UDP 缓冲区大小
sysctl net.core.rmem_max
sysctl net.core.wmem_max
```

许可证

MIT / Apache-2.0
