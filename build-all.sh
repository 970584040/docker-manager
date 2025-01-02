#!/bin/bash

# 创建输出目录
mkdir -p releases

# 定义目标平台
targets=(
    # Linux (x86_64 和 ARM64)
    "x86_64-unknown-linux-gnu"     # Linux Intel/AMD 64位
    "aarch64-unknown-linux-gnu"    # Linux ARM64
    "i686-unknown-linux-gnu"       # Linux 32位
    
    # macOS (Intel 和 Apple Silicon)
    "x86_64-apple-darwin"          # macOS Intel
    "aarch64-apple-darwin"         # macOS M1/M2
    
    # Windows
    "x86_64-pc-windows-gnu"        # Windows 64位
    "i686-pc-windows-gnu"          # Windows 32位
    "aarch64-pc-windows-msvc"      # Windows ARM64 (Surface Pro X, Windows 11 ARM)
) 