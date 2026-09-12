FROM ubuntu:20.04

# 避免交互式安装提示
ENV DEBIAN_FRONTEND=noninteractive

# 更新软件源并安装所需软件包
# 软件版本参考 NOI Linux 2.0 配置
# https://noi.cn/gynoi/jsgz/2021-07-16/732450.shtml
# 不含有 fpc, python2, ddd, emacs, joe 及 GUI 程序 
RUN apt-get update && apt-get install -y \
    unzip \
    curl \
    gcc=4:9.3.0-1ubuntu2 \
    g++=4:9.3.0-1ubuntu2 \
    gdb=9.1-0ubuntu1 \
    vim=2:8.1.2269-1ubuntu5.32 \
    nano=4.8-1ubuntu1.1 \
    python3=3.8.2-0ubuntu2 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -m -s /bin/bash oiph \
    && curl -fsSL http://sxrzh.github.io/oiph/install/latest.sh | bash -s -- --user=oiph   # v0.1.4+

# 切换到普通用户 oiph
USER oiph
WORKDIR /home/oiph

# 容器启动时默认执行 bash（可根据需要修改为其他命令）
CMD ["/bin/bash"]
