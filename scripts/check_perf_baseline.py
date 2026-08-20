#!/usr/bin/env python3
"""性能回归门禁：跑 benches/perf_baseline.rs，判它的结果还合不合理。

背景与 flare-core 那道同源：这个 bench 一直存在，**从来没有任何 CI 跑过它**。
基线建了没人看，就是慢慢腐坏——bench 编不过、跑 panic、或者某个 case 被悄悄
删掉，都不会有任何信号。

它测的全是**本地**路径（内存 store、事件总线、编解码），不需要后端，所以能
直接进 CI。MX-PERF-01 里「sync 千条」这一项就落在
`sync_messages/event_bus_publish_and_drain_1000` 上。

判据三类，全都与机器速度无关或留足量级余量：

  1. **齐全性**：点名的 benchmark 一个都不能少。少了就是判据被悄悄缩水
     （改名、删掉、条件编译掉），这类「什么都没验还输出绿」比红更危险。
  2. **规模比值**：同一轮内「1000 条 ÷ 100 条」这类同形不同量的对比。
     它测的是**复杂度**不是速度——机器快慢同时作用于分子分母，比值稳定。
     这是这份 bench 比 flare-core 那份更好的地方：那边只有两条形态不同的
     路径可比，比值落在噪声里只好撤掉；这边是同一段代码跑 10 倍的量，
     线性就该是 10 倍，退化成 O(n²) 会变成 100 倍，一眼就分得开。
  3. **量级上限**：留 50 倍以上余量的绝对天花板。抓不住 20% 的劣化，
     但能抓住「热路径混进阻塞调用 / 复杂度退化」这种数量级事故。

**不按绝对耗时设紧阈值**：共享 runner 上同一段代码能差好几倍（flare-core 那道
门禁实测 CI 比本机慢 2~3.2 倍）。按本机数字卡紧必然长期红，而
「非阻塞 + 长期红 = 告警彻底失效」这个亏本项目已经吃过一次。

用法：
    python3 scripts/check_perf_baseline.py            # 自己跑 bench 再判
    python3 scripts/check_perf_baseline.py out.txt    # 判已有的 bencher 格式输出
"""

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# criterion 的 --output-format bencher 每行长这样：
#   test group/case ... bench:        1307 ns/iter (+/- 30)
LINE = re.compile(r"^test\s+(\S+)\s+\.\.\.\s+bench:\s+([\d,]+)\s+ns/iter")

# 点名清单。bench 里加了新项不必登记，但**这些少一个就判红**。
REQUIRED = [
    "event_bus_publish_steady_state/0",
    "event_bus_publish_steady_state/1",
    "event_bus_publish_steady_state/10",
    "event_bus_publish_steady_state/100",
    "event_filter/try_recv_matching",
    "message_send/prepare_text_message",
    "message_send/memory_store_save_batch_100",
    "message_send/local_prepare_store_encode_100",
    "message_receive/event_bus_publish_and_drain_100",
    "sync_messages/event_bus_publish_and_drain_1000",
    "event_json_serialization/message_received_payload",
    "event_json_serialization/sync_messages_1000_payload",
    "protocol_codec/decode_data_packet",
]

# (大规模项, 小规模项, 倍数, 上限倍数, 说明)
# 判的是复杂度：量放大 N 倍，耗时该跟着放大约 N 倍。上限取 2N，
# 线性(N)与平方(N²)之间差着两个数量级，2N 这个位置分得干净又不会被噪声碰到。
SCALING = [
    (
        "sync_messages/event_bus_publish_and_drain_1000",
        "message_receive/event_bus_publish_and_drain_100",
        10,
        20.0,
        "同一段「发布 N 条再全部取走」的代码，量从 100 涨到 1000。"
        "线性就该是 10 倍左右（实测 10.5）；升到 20 倍以上说明单条成本随总量增长，"
        "多半是某处退化成了 O(n²)。",
    ),
    (
        "event_bus_publish_steady_state/100",
        "event_bus_publish_steady_state/10",
        10,
        20.0,
        "订阅者从 10 个涨到 100 个，一次 publish 的成本该线性增长（实测 9.6 倍）。"
        "超过 20 倍说明每个订阅者的分发成本本身在随订阅者总数变化。",
    ),
]

# 量级天花板（ns/op），按实测值留 50 倍以上余量。抓数量级事故，不是抖动。
CEILINGS = {
    "protocol_codec/decode_data_packet": 10_000,  # 实测 ~64
    "message_send/prepare_text_message": 100_000,  # 实测 ~906
    "event_bus_publish_steady_state/1": 100_000,  # 实测 ~1,307
    "event_filter/try_recv_matching": 100_000,  # 实测 ~1,225
    "message_send/local_prepare_store_encode_100": 10_000_000,  # 实测 ~182,367
    "message_receive/event_bus_publish_and_drain_100": 5_000_000,  # 实测 ~88,190
    "sync_messages/event_bus_publish_and_drain_1000": 50_000_000,  # 实测 ~922,398
    "event_json_serialization/sync_messages_1000_payload": 150_000_000,  # 实测 ~3,205,471
}


# cargo 把编译进度写 stderr、bench 结果写 stdout。出问题时两边都要看：
# 第一版只在「returncode != 0」时打印它们，结果 CI 上遇到「exit 0 但 stdout 为空」
# 就只剩一句「criterion 的输出格式变了？」——那是猜测，不是诊断，把人往错方向带。
_LAST_STDERR = ""


def run_bench() -> str:
    global _LAST_STDERR
    proc = subprocess.run(
        [
            "cargo",
            "bench",
            "--bench",
            "perf_baseline",
            "--",
            "--output-format",
            "bencher",
        ],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    _LAST_STDERR = proc.stderr
    if proc.returncode != 0:
        print("✗ bench 没跑起来（这本身就是回归：基线不可执行）", file=sys.stderr)
        print(proc.stdout[-3000:], file=sys.stderr)
        print(proc.stderr[-3000:], file=sys.stderr)
        sys.exit(1)
    return proc.stdout


def parse(text: str) -> dict:
    out = {}
    for line in text.splitlines():
        m = LINE.match(line.strip())
        if m:
            out[m.group(1)] = float(m.group(2).replace(",", ""))
    return out


def main() -> int:
    raw = Path(sys.argv[1]).read_text() if len(sys.argv) > 1 else run_bench()
    got = parse(raw)

    if not got:
        print("✗ 一条 bencher 格式的结果都没解析出来", file=sys.stderr)
        print(
            "  cargo 退出码是 0，所以不是编译失败。可能是 bench 压根没被执行\n"
            "  （目标被 required-features 之类过滤掉，cargo 只 Finished 不 Running），\n"
            "  也可能是 criterion 的输出格式变了。下面是 cargo 自己说的话——"
            "别再靠猜：",
            file=sys.stderr,
        )
        print("  --- cargo stdout（原样，前 1500 字）---", file=sys.stderr)
        print(raw[:1500] or "  (空)", file=sys.stderr)
        print("  --- cargo stderr（尾 2500 字）---", file=sys.stderr)
        print(_LAST_STDERR[-2500:] or "  (空)", file=sys.stderr)
        return 1

    # 先把数字打出来：判红时 stdout/stderr 交错，表格放后面会被冲散。
    print("性能基线（本轮实测）：")
    for name in sorted(got):
        print(f"  {name:<52} {got[name]:>14,.0f} ns/op")
    print()

    problems = []

    missing = [n for n in REQUIRED if n not in got]
    if missing:
        problems.append(
            "  ✗ 少了这些 benchmark：\n"
            + "\n".join(f"      {n}" for n in missing)
            + "\n    改名或删项都要同步改本文件的 REQUIRED，"
            "否则门禁会在「什么都没验」的状态下输出绿。"
        )

    for big, small, factor, ceiling, why in SCALING:
        if big not in got or small not in got:
            continue  # 齐全性那条已经报过了
        ratio = got[big] / got[small]
        if ratio > ceiling:
            problems.append(
                f"  ✗ {big}\n"
                f"    ÷ {small} = {ratio:.1f}，超过上限 {ceiling}（量放大了 {factor} 倍）\n"
                f"    {why}"
            )

    for name, ceiling in CEILINGS.items():
        if name not in got:
            continue
        if got[name] > ceiling:
            problems.append(
                f"  ✗ {name}: {got[name]:,.0f} ns/op 超过量级上限 {ceiling:,} ns/op\n"
                f"    上限留了 50 倍以上余量，撞上它说明是数量级事故"
                f"（热路径混进阻塞调用、复杂度退化），不是 runner 抖动。"
            )

    if problems:
        print("性能回归：", file=sys.stderr)
        print("\n".join(problems), file=sys.stderr)
        return 1

    print(
        f"  ✓ {len(REQUIRED)} 项齐全，{len(SCALING)} 条规模比值与 "
        f"{len(CEILINGS)} 条量级上限均通过"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
