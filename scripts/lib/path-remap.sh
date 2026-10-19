# 打包脚本共用：去掉二进制里的本机绝对路径。被 dev-bundle.sh / release-bundle.sh source。
#
# rustc 会把依赖源码、工具链、项目目录、构建产物目录（build.rs 的 OUT_DIR）的绝对路径
# 编进 panic 位置信息和调试信息，strip 去不掉，分发出去就会暴露用户名和目录结构。
# 用 cargo --config 追加到 target.<host>.rustflags，而不是设置 RUSTFLAGS：
# RUSTFLAGS 会整体覆盖 ~/.cargo/config.toml 里的 rustflags（如 lld 链接器参数），--config 则是合并。
#
# 用法（调用方需先 cd 到仓库根目录）：
#   source scripts/lib/path-remap.sh
#   cargo build ... "${PATH_REMAP_ARGS[@]}"
#   assert_no_home_paths path/to/binary

_remap_host="$(rustc -vV | sed -n 's/^host: //p')"
_remap_target_dir="$(cargo metadata --no-deps --format-version 1 | python3 -c 'import sys,json;print(json.load(sys.stdin)["target_directory"])')"
# 顺序有意义：rustc 取最后一个匹配的前缀，具体路径放在后面，避免被 $HOME 级前缀抢先命中
PATH_REMAP_ARGS=(--config "target.$_remap_host.rustflags=[\
\"--remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo\", \
\"--remap-path-prefix=${RUSTUP_HOME:-$HOME/.rustup}=/rustup\", \
\"--remap-path-prefix=$_remap_target_dir=/target\", \
\"--remap-path-prefix=$PWD=/mymac\"]")
unset _remap_host _remap_target_dir

# 自检：二进制里仍含本机 home 路径就失败退出，不产出可分发的包
assert_no_home_paths() {
    local bin="$1" leaks
    leaks="$(strings -a "$bin" | grep -cF "$HOME/" || true)"
    if [ "$leaks" != "0" ]; then
        echo "二进制中仍有 $leaks 处本机路径（$HOME/...），已中止打包：" >&2
        strings -a "$bin" | grep -F "$HOME/" | head -5 >&2
        exit 1
    fi
    echo "路径自检通过：二进制中无本机 home 路径"
}
