#!/bin/bash

# 创建输出目录
mkdir -p target/releases

# 定义目标平台
targets=(
    # Linux
    "x86_64-unknown-linux-gnu"     # Linux Intel/AMD 64位
    "aarch64-unknown-linux-gnu"    # Linux ARM64
    "i686-unknown-linux-gnu"       # Linux 32位
    
    # macOS
    "x86_64-apple-darwin"          # macOS Intel
    "aarch64-apple-darwin"         # macOS ARM
)

# 程序名称
program_name="docker-manager"

# 为每个目标编译
for target in "${targets[@]}"; do
    echo "正在编译 $target..."
    
    # 添加目标支持
    rustup target add "$target"
    
    # 编译
    cargo build --release --target "$target"
    
    # 检查编译是否成功
    if [ $? -ne 0 ]; then
        echo "编译 $target 失败"
        continue
    fi
    
    # 创建目标目录
    mkdir -p "target/releases/$target"
    
    # 复制编译结果到发布目录
    cp "target/$target/release/$program_name" "target/releases/$target/"
    
    echo "完成 $target"
    echo "-------------------"
done

# 压缩每个版本
cd target/releases
for target in "${targets[@]}"; do
    if [ -d "$target" ]; then  # 只处理成功编译的目标
        tar -czf "${program_name}-${target}.tar.gz" "$target"
    fi
done

echo "所有版本编译完成！"
echo "编译结果在 target/releases 目录中"

# 显示编译结果统计
echo -e "\n编译结果统计："
echo "成功的版本："
ls -1 ./*.tar.gz 2>/dev/null | sed 's/\.\///' || echo "无" 