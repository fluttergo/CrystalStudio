# 本文件已废弃（2026-11-05），**故意不含 FROM** —— 它不可构建。
#
# 唯一有效的 Dockerfile 是 `tools/miredit/docker/Dockerfile`
# （构建上下文必须是 `tools/`，因为 `mir2-assets` 依赖 `../../../wil2atlas`）。
#
# 为什么不直接删掉：删了旧脚本只会得到 `no such file`，丢失「该去哪」的指路信息；
# 保留一个**显式不可构建**的文件，报错本身就是指路。
#
# 也不要改回 `FROM scratch`：那是合法的空基础镜像，`docker build` 会**成功**产出一个
# 空镜像（无 CMD/ENTRYPOINT），要到 `docker run` 才报错 —— 那是假 fail-fast，比直接失败更容易被误用。
#
# 用法：`docker build -f tools/miredit/docker/Dockerfile tools/`
