import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { en } from "../lib/i18n/en";
import { zh } from "../lib/i18n/zh";
import { translate } from "../lib/i18n";
import { formatBytes, pathTail, samePath } from "../lib/format";
import { protectionState } from "../lib/protection";
import { project, worktrees } from "./fixtures";
import { FileList } from "../features/snapshots/FileList";
import { I18nProvider } from "../lib/i18n";
import type { SnapshotFile } from "../lib/types";

// Identifiers emitted by the Rust core that the UI must be able to explain.
const RISK_RULES = [
  "unprotectedWorkUnreachable",
  "worktreeUnreachableProtected",
  "worktreeUnreachable",
  "staleProtection",
  "autoProtectFailing",
  "storageLow",
  "budgetNearlyFull",
  "unprotectedFiles",
  "massDeletion",
  "snapshotIntegrity",
];
const SKIP_REASONS = ["untrackedDisabled", "excluded", "sensitive", "tooLarge", "unstable", "unreadable", "gitIgnored", "unsafePath", "submodule", "unsupportedType", "budgetExceeded"];
const EVIDENCE = ["gitReportsPrunable", "locked", "pathNotFound", "volumeMounted", "volumeUnavailable", "pointerFoundElsewhere", "gitPointerMismatch", "statusFailed", "accessDenied", "removedFromGitMetadata", "directoryStillExists"];
const ERROR_CODES = ["git_not_found", "git_failed", "git_timeout", "cancelled", "not_a_repository", "bare_repository", "unsafe_path", "project_not_found", "snapshot_not_found", "already_registered", "integrity", "invalid_target", "recovery_aborted", "insufficient_space", "budget_exceeded", "invalid_manifest", "invalid_setting", "busy", "io", "db"];

describe("i18n", () => {
  it("has the same keys in both languages and no empty Chinese strings except intentional ones", () => {
    expect(Object.keys(zh).sort()).toEqual(Object.keys(en).sort());
    const empty = Object.entries(zh).filter(([, v]) => v === "").map(([k]) => k);
    expect(empty).toEqual(["snapshot.deleteShared"]);
  });

  it("explains every backend identifier in both languages", () => {
    const need = [
      ...RISK_RULES.flatMap((r) => [`risk.${r}.title`, `risk.${r}.explain`, `risk.${r}.action`]),
      ...SKIP_REASONS.map((s) => `skip.${s}`),
      ...EVIDENCE.map((e) => `evidence.${e}`),
      ...ERROR_CODES.map((c) => `error.${c}`),
    ];
    for (const k of need) {
      expect(en, k).toHaveProperty([k]);
      expect(zh, k).toHaveProperty([k]);
    }
  });

  it("keeps interpolation parameters consistent between languages", () => {
    const params = (s: string) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();
    for (const k of Object.keys(en) as (keyof typeof en)[]) {
      expect(params(zh[k]), k).toEqual(params(en[k]));
    }
  });

  it("interpolates parameters", () => {
    expect(translate("en", "snapshot.filesCount", { count: 3 })).toBe("3 files");
    expect(translate("zh-CN", "snapshot.filesCount", { count: 3 })).toBe("3 个文件");
  });
});

describe("format & protection", () => {
  it("formats bytes and paths", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(pathTail("C:\\a\\b\\c\\d", 2)).toBe("…\\c\\d");
    expect(samePath("C:/A/b/", "c:\\a\\B")).toBe(true);
  });

  it("derives protection states from observed facts only", () => {
    const [main, cache, gone] = worktrees;
    expect(protectionState(main, project, 120)).toBe("protected");
    expect(protectionState(cache, project, 120)).toBe("stale");
    expect(protectionState({ ...cache, lastProtectedAt: null }, project, 120)).toBe("unprotected");
    expect(protectionState(gone, project, 120)).toBe("missing");
    expect(protectionState({ ...main, dirty: false }, project, 120)).toBe("clean");
    expect(protectionState(main, { ...project, monitoring: "paused" }, 120)).toBe("paused");
    expect(protectionState({ ...gone, diagnosis: "volumeUnavailable" }, project, 120)).toBe("unknown");
  });
});

describe("large lists", () => {
  it("virtualizes thousands of files", () => {
    const files: SnapshotFile[] = Array.from({ length: 5000 }, (_, i) => ({
      path: `src/generated/file-${i}.ts`,
      origPath: null,
      category: "untracked",
      x: "?",
      y: "?",
      kind: "file",
      wtState: "present",
      wtBlob: "a".repeat(64),
      wtSize: 10,
      indexBlob: null,
      indexOid: null,
      headOid: null,
      skipReason: null,
      linkTarget: null,
      lfsPointer: false,
      binary: false,
    }));
    const { container } = render(
      <I18nProvider locale="en">
        <FileList files={files} />
      </I18nProvider>,
    );
    const rows = container.querySelectorAll(".file-row");
    expect(rows.length).toBeGreaterThan(0);
    expect(rows.length).toBeLessThan(200);
  });
});
