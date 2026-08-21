// Отчёт, который не жалко прочитать целиком.
//
// Прогон — восемьдесят с лишним замеров; список из восьмидесяти строк
// «ok» не сообщает ничего, чего не сообщила бы одна. Печатаются только
// провалы, по строке на каждый, с именем фазы, съевшей бюджет. Всё
// остальное — каждый спан, каждая студия, стены и перцентили — уходит
// в last-run.json и ждёт там, пока действительно понадобится.
//
// PERF_VERBOSE=1 печатает таблицу целиком.
// PERF_GATE=regressions роняет прогон только на новых или ухудшившихся
// провалах относительно baseline.json — режим для будущего CI.

import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import type { FullResult, Reporter, TestCase, TestResult } from "@playwright/test/reporter";

const HERE = dirname(fileURLToPath(import.meta.url));
const LAST_RUN = join(HERE, "last-run.json");
const BASELINE = join(HERE, "baseline.json");

interface Row {
  study: string;
  scenario: string;
  worstBlockMs: number;
  worstPhase: string | null;
  worstLongFrameMs: number;
  wallMs: number;
  plansPerFrame: number;
  heldTiles: number;
  heldBytes: number;
  phases: Record<string, { count: number; totalMs: number; maxMs: number; p95Ms: number }>;
  ok: boolean;
  /** Первая строка ошибки — для провалов без измерения (упавших до него). */
  error?: string;
}

type Baseline = Record<string, number>;

export default class PerfReporter implements Reporter {
  private readonly rows: Row[] = [];
  private failures = 0;
  private passes = 0;

  onTestEnd(test: TestCase, result: TestResult): void {
    const ok = result.status === "passed";
    if (ok) this.passes += 1;
    else this.failures += 1;

    const attachment = result.attachments.find((item) => item.name === "maps2-perf");
    const measured = attachment?.body
      ? (JSON.parse(attachment.body.toString()) as Omit<Row, "ok">)
      : null;
    if (measured) {
      this.rows.push({ ...measured, ok });
      return;
    }
    // Тест, упавший до замера (студия не открылась, реестр разошёлся с
    // лабой): такое надо показать, а не потерять из-за отсутствия
    // вложения.
    if (!ok) {
      this.rows.push({
        study: test.parent.title === test.parent.parent?.title ? test.title : test.parent.title,
        scenario: test.title,
        worstBlockMs: 0,
        worstPhase: null,
        worstLongFrameMs: 0,
        wallMs: 0,
        plansPerFrame: 0,
        heldTiles: 0,
        heldBytes: 0,
        phases: {},
        ok: false,
        error: firstLine(result.error?.message),
      });
    }
  }

  onEnd(result: FullResult): { status: FullResult["status"] } | void {
    writeJson(LAST_RUN, { at: new Date().toISOString(), rows: this.rows });
    if (process.env.PERF_BASELINE === "write") writeBaseline(this.rows);
    const baseline = readBaseline();
    const failed = this.rows.filter((row) => !row.ok);
    const worst = this.rows
      .filter((row) => row.ok)
      .reduce<Row | null>((max, row) => (!max || row.worstBlockMs > max.worstBlockMs ? row : max), null);

    const total = this.passes + this.failures;
    const head = `perf ${this.passes}/${total} within budget`;
    const tail = worst ? ` · worst pass: ${worst.study} ${worst.scenario} ${fixed(worst.worstBlockMs)}` : "";
    process.stdout.write(`\n${head}${tail}\n`);

    let regressions = 0;
    for (const row of failed) {
      const key = `${row.study}/${row.scenario}`;
      const before = baseline[key];
      const known = before !== undefined && row.worstBlockMs <= before * 1.1;
      if (!known) regressions += 1;
      process.stdout.write(`${known ? "KNOWN" : "NEW  "} ${line(row)}\n`);
    }

    if (process.env.PERF_VERBOSE) {
      for (const row of this.rows.filter((item) => item.ok)) {
        process.stdout.write(`  ok  ${line(row)}\n`);
      }
    }
    process.stdout.write(`→ ${relative(LAST_RUN)}\n`);

    if (process.env.PERF_GATE === "regressions") {
      return { status: regressions > 0 ? "failed" : "passed" };
    }
    return { status: result.status };
  }
}

function line(row: Row): string {
  const where = `${row.study.padEnd(22)} ${row.scenario.slice(0, 34).padEnd(34)}`;
  if (row.error) return `${where} ${row.error}`;
  const phase = row.worstPhase ? ` ${row.worstPhase}` : "";
  const at = row.worstDetail ? ` ${row.worstDetail}` : "";
  const busiest = topPhase(row);
  return `${where} ${fixed(row.worstBlockMs)}${phase}${at}${busiest}`;
}

/// Худший вызов говорит, где рвётся; самая дорогая фаза за проход
/// говорит, куда смотреть. Обычно это одно и то же — а когда нет, это
/// и есть интересный случай.
///
/// Считаются только фазы, которые действительно держат поток. `fetch`
/// идёт десятками параллельно, и сумма его длительностей — это не
/// потраченное время, а пятьсот минут ожидания за полторы секунды.
const BLOCKING = new Set(["render", "debug", "demand", "decode", "clamp", "labels"]);

function topPhase(row: Row): string {
  const entries = Object.entries(row.phases).filter(([name]) => BLOCKING.has(name));
  if (entries.length === 0) return "";
  const [name, stats] = entries.sort((a, b) => b[1].totalMs - a[1].totalMs)[0]!;
  return `  ${name} ×${stats.count} (${fixed(stats.totalMs)})`;
}

function fixed(ms: number): string {
  return `${ms.toFixed(1)}ms`;
}

function firstLine(message: string | undefined): string {
  return (message ?? "провал без сообщения").split("\n")[0]!.slice(0, 90);
}

/// Снимок известных провалов, чтобы новая просадка была видна среди
/// старых. Записывается по требованию, а не сама: базовая линия,
/// обновляющаяся каждым прогоном, объявляет любое замедление нормой.
function writeBaseline(rows: Row[]): void {
  const baseline: Baseline = {};
  for (const row of rows) {
    if (!row.ok) baseline[`${row.study}/${row.scenario}`] = row.worstBlockMs;
  }
  writeJson(BASELINE, baseline);
  process.stdout.write(`baseline: ${Object.keys(baseline).length} известных провалов → ${relative(BASELINE)}\n`);
}

function readBaseline(): Baseline {
  if (!existsSync(BASELINE)) return {};
  try {
    return JSON.parse(readFileSync(BASELINE, "utf8")) as Baseline;
  } catch {
    return {};
  }
}

function writeJson(path: string, value: unknown): void {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, `${JSON.stringify(value, null, 1)}\n`);
}

function relative(path: string): string {
  return path.replace(`${process.cwd()}/`, "");
}
