# Docker 容器监控与自动重启工具

这是一个用 Rust 编写的 Docker 容器监控工具，它可以自动监控和重启意外停止的 Docker 容器。

## 主要功能

1. **容器监控**
   - 实时监控所有 Docker 容器的状态
   - 自动检测容器停止事件
   - 支持监控容器的环境变量、端口映射和挂载点

2. **自动重启**
   - 当容器意外停止时自动重启
   - 保持原有容器的所有配置（环境变量、端口映射、挂载点等）
   - 智能重试机制，避免频繁重启

3. **Web 界面**
   - 提供简洁的 Web 界面查看容器状态
   - 实时显示容器运行状态
   - 查看容器详细配置信息

## 技术栈

- Rust
- Bollard (Docker API 客户端)
- Axum (Web 框架)
- Tokio (异步运行时)

## 使用方法

1. **启动服务**
   ```bash
   cargo run
   ```

2. **访问 Web 界面**
   - 打开浏览器访问 `http://localhost:3000`
   - 如果 3000 端口被占用，程序会自动尝试 3001-3009 端口

## 配置说明

- 程序会自动监控所有正在运行的容器
- 当容器停止时，会尝试使用原始配置重启容器
- 重启时会保持：
  - 环境变量
  - 端口映射
  - 卷挂载
  - 网络设置
  - 其他 Docker 运行参数

## 交叉编译指南

### 前置要求

1. **安装 Rust 和 Cargo**
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

2. **安装目标平台支持**
```bash
# Linux 平台
rustup target add x86_64-unknown-linux-gnu    # Linux 64位
rustup target add aarch64-unknown-linux-gnu   # Linux ARM64
rustup target add i686-unknown-linux-gnu      # Linux 32位

# Windows 平台
rustup target add x86_64-pc-windows-gnu       # Windows 64位
rustup target add i686-pc-windows-gnu         # Windows 32位
rustup target add aarch64-pc-windows-msvc     # Windows ARM64

# macOS 平台
rustup target add x86_64-apple-darwin         # macOS Intel
rustup target add aarch64-apple-darwin        # macOS ARM
```

### 安装交叉编译工具链

#### macOS 上安装
```bash
# 安装 Homebrew（如果未安装）
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"

# 安装交叉编译工具
brew install mingw-w64                        # Windows 交叉编译支持
brew install FiloSottile/musl-cross/musl-cross
brew install aarch64-linux-gnu-binutils       # Linux ARM64 支持
brew install x86_64-linux-gnu-binutils       # Linux x86_64 支持
brew install i686-linux-gnu-binutils         # Linux 32位支持
```

#### Linux (Ubuntu/Debian) 上安装
```bash
# 安装交叉编译工具
sudo apt-get update
sudo apt-get install -y \
    gcc-aarch64-linux-gnu \
    gcc-i686-linux-gnu \
    gcc-x86-64-linux-gnu \
    mingw-w64
```

#### Linux (Fedora/RHEL) 上安装
```bash
sudo dnf install -y \
    gcc-aarch64-linux-gnu \
    mingw64-gcc \
    mingw32-gcc
```

### 编译

1. **使用编译脚本**
```bash
# 添加执行权限
chmod +x build-all.sh

# 运行编译脚本
./build-all.sh
```

2. **手动编译特定平台**
```bash
# Linux x86_64
cargo build --release --target x86_64-unknown-linux-gnu

# Linux ARM64
cargo build --release --target aarch64-unknown-linux-gnu

# Windows x64
cargo build --release --target x86_64-pc-windows-gnu

# macOS ARM (M1/M2)
cargo build --release --target aarch64-apple-darwin
```

### 编译产物

编译完成后，可执行文件将位于 `releases` 目录下：

```
releases/
├── docker-manager-x86_64-unknown-linux-gnu.tar.gz     # Linux 64位
├── docker-manager-aarch64-unknown-linux-gnu.tar.gz    # Linux ARM64
├── docker-manager-i686-unknown-linux-gnu.tar.gz       # Linux 32位
├── docker-manager-x86_64-pc-windows-gnu.zip          # Windows 64位
├── docker-manager-i686-pc-windows-gnu.zip            # Windows 32位
├── docker-manager-aarch64-pc-windows-msvc.zip        # Windows ARM64
├── docker-manager-x86_64-apple-darwin.tar.gz         # macOS Intel
└── docker-manager-aarch64-apple-darwin.tar.gz        # macOS ARM
```

### 注意事项

1. Windows ARM64 (aarch64-pc-windows-msvc) 版本需要在 Windows 环境下使用 MSVC 工具链编译
2. 某些依赖可能需要特定的系统库
3. 如果遇到编译错误，请确保已安装所有必要的依赖
4. 在不同操作系统上进行交叉编译可能需要不同的工具链配置

### 常见问题

1. **编译 Windows 版本时出错**
   - 确保已正确安装 mingw-w64
   - 检查 `.cargo/config.toml` 中的链接器配置

2. **找不到链接器**
   - 确保已安装对应平台的工具链
   - 检查环境变量是否正确设置

3. **缺少依赖库**
   - 根据错误信息安装缺少的系统库
   - 对于 Linux 目标，可能需要安装额外的开发包

## 注意事项

1. 需要有 Docker daemon 的访问权限
2. 建议使用 systemd 或其他工具确保本程序持续运行
3. 日志会输出到标准输出，建议配合日志收集工具使用

## 贡献

欢迎提交 Issue 和 Pull Request！

## 许可证

MIT License
