from __future__ import annotations

from coworker.skills.loader import SkillLoader

VALID_SKILL = """\
---
name: test-skill
description: 测试时使用此技能
---

## 技能内容

这是技能的主体内容，包含具体指引。
"""

SKILL_NO_FRONTMATTER = "just some text without frontmatter"

SKILL_MISSING_NAME = """\
---
description: no name here
---

body
"""

SKILL_WITH_VERSION_AND_METADATA = """\
---
name: lark-base
version: 1.2.0
description: "当需要用 lark-cli 操作飞书多维表格（Base）时调用"
metadata:
  requires:
    bins: ["lark-cli"]
  cliHelp: "lark-cli base --help"
---

# base
"""

SKILL_INVALID_YAML = """\
---
name: bad-yaml
description: [unclosed
---

body
"""


class TestSkillLoader:
    def _make_skill(self, tmp_path, skill_name: str, content: str):
        d = tmp_path / skill_name
        d.mkdir()
        (d / "SKILL.md").write_text(content, encoding="utf-8")

    def test_load_valid_skill(self, tmp_path):
        self._make_skill(tmp_path, "test-skill", VALID_SKILL)
        loader = SkillLoader(str(tmp_path))
        loader.load_all()
        assert "test-skill" in loader._skills

    def test_skill_fields_parsed_correctly(self, tmp_path):
        self._make_skill(tmp_path, "test-skill", VALID_SKILL)
        loader = SkillLoader(str(tmp_path))
        loader.load_all()
        skill = loader._skills["test-skill"]
        assert skill.description == "测试时使用此技能"
        assert "技能的主体内容" in skill.body

    def test_skip_file_without_frontmatter(self, tmp_path):
        self._make_skill(tmp_path, "bad-skill", SKILL_NO_FRONTMATTER)
        loader = SkillLoader(str(tmp_path))
        loader.load_all()
        assert len(loader._skills) == 0
        warnings = loader.consume_skill_load_warnings()
        assert len(warnings) == 1
        assert "缺少 frontmatter" in warnings[0]

    def test_skip_skill_missing_name(self, tmp_path):
        self._make_skill(tmp_path, "nameless", SKILL_MISSING_NAME)
        loader = SkillLoader(str(tmp_path))
        loader.load_all()
        assert len(loader._skills) == 0
        warnings = loader.consume_skill_load_warnings()
        assert len(warnings) == 1
        assert "缺少 name" in warnings[0]

    def test_load_multiple_skills(self, tmp_path):
        for i in range(3):
            content = f"---\nname: skill-{i}\ndescription: desc {i}\n---\nbody {i}"
            self._make_skill(tmp_path, f"skill-{i}", content)
        loader = SkillLoader(str(tmp_path))
        loader.load_all()
        assert len(loader._skills) == 3

    def test_format_for_prompt_empty(self, tmp_path):
        loader = SkillLoader(str(tmp_path / "nonexistent"))
        assert loader.format_for_prompt() == ""

    def test_format_for_prompt_not_contains_skill_content(self, tmp_path):
        self._make_skill(tmp_path, "test-skill", VALID_SKILL)
        loader = SkillLoader(str(tmp_path))
        output = loader.format_for_prompt()
        assert "test-skill" in output
        assert "测试时使用此技能" in output
        assert "技能的主体内容" not in output

    def test_nonexistent_skills_dir(self, tmp_path):
        loader = SkillLoader(str(tmp_path / "missing"))
        loader.load_all()  # should not raise
        assert len(loader._skills) == 0

    def test_version_and_metadata_parsed(self, tmp_path):
        self._make_skill(tmp_path, "lark-base", SKILL_WITH_VERSION_AND_METADATA)
        loader = SkillLoader(str(tmp_path))
        loader.load_all()
        skill = loader._skills["lark-base"]
        assert skill.version == "1.2.0"
        assert skill.metadata["requires"]["bins"] == ["lark-cli"]
        assert skill.metadata["cliHelp"] == "lark-cli base --help"

    def test_quoted_description_parsed(self, tmp_path):
        self._make_skill(tmp_path, "lark-base", SKILL_WITH_VERSION_AND_METADATA)
        loader = SkillLoader(str(tmp_path))
        loader.load_all()
        skill = loader._skills["lark-base"]
        assert "lark-cli" in skill.description

    def test_skip_invalid_yaml_frontmatter(self, tmp_path):
        self._make_skill(tmp_path, "bad-yaml", SKILL_INVALID_YAML)
        loader = SkillLoader(str(tmp_path))
        loader.load_all()
        assert len(loader._skills) == 0
        warnings = loader.consume_skill_load_warnings()
        assert len(warnings) == 1
        assert "YAML frontmatter 解析失败" in warnings[0]

    def test_persistent_warning_not_reemitted_every_load(self, tmp_path):
        self._make_skill(tmp_path, "bad-yaml", SKILL_INVALID_YAML)
        loader = SkillLoader(str(tmp_path))
        loader.load_all()
        first = loader.consume_skill_load_warnings()
        assert len(first) == 1

        loader.load_all()
        second = loader.consume_skill_load_warnings()
        assert second == []

    def test_duplicate_skill_name_emits_warning(self, tmp_path):
        self._make_skill(tmp_path, "skill-a", VALID_SKILL)
        self._make_skill(
            tmp_path,
            "skill-b",
            VALID_SKILL.replace("test-skill", "test-skill").replace("测试时使用此技能", "另一个定义"),
        )
        loader = SkillLoader(str(tmp_path))
        loader.load_all()
        warnings = loader.consume_skill_load_warnings()
        assert len(loader._skills) == 1
        assert any("重名" in warning for warning in warnings)

    def test_default_version_is_none(self, tmp_path):
        self._make_skill(tmp_path, "test-skill", VALID_SKILL)
        loader = SkillLoader(str(tmp_path))
        loader.load_all()
        assert loader._skills["test-skill"].version is None

    def test_default_metadata_is_empty_dict(self, tmp_path):
        self._make_skill(tmp_path, "test-skill", VALID_SKILL)
        loader = SkillLoader(str(tmp_path))
        loader.load_all()
        assert loader._skills["test-skill"].metadata == {}
