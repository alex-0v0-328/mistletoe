// SessionStart 钩子：每次会话开始（含 resume / clear / compact 之后）把两个常驻 skill 注入上下文。
// - constitution：四象限协作协议（原为项目根目录的 SKILL.md，已迁到 .claude/skills/constitution/）
// - i-have-adhd：ADHD 友好输出规则（来自 github.com/ayghri/i-have-adhd @ 839872f）
// 任何异常都静默退出 0，绝不阻塞会话启动。

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

// 按注入顺序列出 skill；constitution 优先级最高，放在最前
const SKILLS = ["constitution", "i-have-adhd"];

try {
  const skillsDir = path.join(path.dirname(fileURLToPath(import.meta.url)), "..", "skills");
  const parts = [];

  for (const name of SKILLS) {
    const file = path.join(skillsDir, name, "SKILL.md");
    if (!fs.existsSync(file)) continue;
    // 去掉开头的 YAML frontmatter，只注入正文
    const body = fs
      .readFileSync(file, "utf8")
      .replace(/^---[^\S\r\n]*\r?\n[\s\S]*?\r?\n---[^\S\r\n]*(?:\r?\n|$)/, "")
      .trim();
    parts.push(`<skill name="${name}">\n${body}\n</skill>`);
  }

  if (parts.length > 0) {
    process.stdout.write(
      "ALWAYS-ON SKILLS (project Mistletoe). Apply every rule below to every response. " +
        '"stop adhd mode" turns off i-have-adhd for this session only.\n\n' +
        parts.join("\n\n") +
        "\n",
    );
  }
} catch {
  process.exit(0);
}
