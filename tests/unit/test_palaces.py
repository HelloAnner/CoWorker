from __future__ import annotations

from coworker.palaces.loader import PalaceLoader, _as_str_list

VALID_PALACE = """\
---
name: product-bug
when_to_attach: 用户反馈示例产品缺陷，需走问题提交流程
critical_skills: [bug-create]
related_skills: [issue-tracker, product-config]
memory_tags: [product, bug]
---

# 速记卡

这里是领域速记卡正文。
"""

PALACE_COMMA_LISTS = """\
---
name: comma-palace
when_to_attach: 测试逗号分隔
critical_skills: a, b
memory_tags: tag1, tag2
---
body
"""

PALACE_NO_FRONTMATTER = "just text"

PALACE_MISSING_NAME = """\
---
when_to_attach: no name
---
body
"""

PALACE_INVALID_YAML = """\
---
name: bad
when_to_attach: [unclosed
---
body
"""


def _make_palace(tmp_path, dir_name: str, content: str) -> None:
    d = tmp_path / dir_name
    d.mkdir()
    (d / "PALACE.md").write_text(content, encoding="utf-8")


class TestAsStrList:
    def test_none(self):
        assert _as_str_list(None) == []

    def test_yaml_list(self):
        assert _as_str_list(["a", "b"]) == ["a", "b"]

    def test_comma_string(self):
        assert _as_str_list("a, b ,c") == ["a", "b", "c"]

    def test_strips_empty(self):
        assert _as_str_list([" a ", "", "b"]) == ["a", "b"]


class TestPalaceLoader:
    def test_load_valid_palace(self, tmp_path):
        _make_palace(tmp_path, "product-bug", VALID_PALACE)
        loader = PalaceLoader(str(tmp_path))
        loader.load_all()
        assert "product-bug" in loader._palaces

    def test_fields_parsed(self, tmp_path):
        _make_palace(tmp_path, "product-bug", VALID_PALACE)
        loader = PalaceLoader(str(tmp_path))
        loader.load_all()
        p = loader.get("product-bug")
        assert p is not None
        assert p.when_to_attach == "用户反馈示例产品缺陷，需走问题提交流程"
        assert p.critical_skills == ["bug-create"]
        assert p.related_skills == ["issue-tracker", "product-config"]
        assert p.memory_tags == ["product", "bug"]
        assert "领域速记卡正文" in p.body

    def test_comma_separated_lists(self, tmp_path):
        _make_palace(tmp_path, "comma-palace", PALACE_COMMA_LISTS)
        loader = PalaceLoader(str(tmp_path))
        loader.load_all()
        p = loader.get("comma-palace")
        assert p is not None
        assert p.critical_skills == ["a", "b"]
        assert p.memory_tags == ["tag1", "tag2"]

    def test_skip_no_frontmatter(self, tmp_path):
        _make_palace(tmp_path, "bad", PALACE_NO_FRONTMATTER)
        loader = PalaceLoader(str(tmp_path))
        loader.load_all()
        assert len(loader._palaces) == 0
        warnings = loader.consume_load_warnings()
        assert len(warnings) == 1
        assert "缺少 frontmatter" in warnings[0]

    def test_skip_missing_name(self, tmp_path):
        _make_palace(tmp_path, "nameless", PALACE_MISSING_NAME)
        loader = PalaceLoader(str(tmp_path))
        loader.load_all()
        assert len(loader._palaces) == 0
        assert any("缺少 name" in w for w in loader.consume_load_warnings())

    def test_skip_invalid_yaml(self, tmp_path):
        _make_palace(tmp_path, "bad-yaml", PALACE_INVALID_YAML)
        loader = PalaceLoader(str(tmp_path))
        loader.load_all()
        assert len(loader._palaces) == 0
        assert any("YAML frontmatter 解析失败" in w for w in loader.consume_load_warnings())

    def test_nonexistent_dir(self, tmp_path):
        loader = PalaceLoader(str(tmp_path / "missing"))
        loader.load_all()  # should not raise
        assert len(loader._palaces) == 0

    def test_format_for_prompt_thin_registry(self, tmp_path):
        _make_palace(tmp_path, "product-bug", VALID_PALACE)
        loader = PalaceLoader(str(tmp_path))
        output = loader.format_for_prompt()
        # registry shows name + when_to_attach, NOT the card body
        assert "product-bug" in output
        assert "用户反馈示例产品缺陷" in output
        assert "领域速记卡正文" not in output

    def test_format_for_prompt_empty(self, tmp_path):
        loader = PalaceLoader(str(tmp_path / "nonexistent"))
        assert loader.format_for_prompt() == ""

    def test_duplicate_name_emits_warning(self, tmp_path):
        _make_palace(tmp_path, "a", VALID_PALACE)
        _make_palace(tmp_path, "b", VALID_PALACE)
        loader = PalaceLoader(str(tmp_path))
        loader.load_all()
        assert len(loader._palaces) == 1
        assert any("重名" in w for w in loader.consume_load_warnings())


class TestPalaceCriticalSkills:
    """Critical skills referenced by a palace should resolve through SkillLoader."""

    def test_critical_skill_exists(self, tmp_path):
        from coworker.skills.loader import SkillLoader

        content = VALID_PALACE.replace("bug-create", "palace-creator")
        _make_palace(tmp_path, "product-bug", content)
        skills_dir = tmp_path / "skills"
        skill_dir = skills_dir / "palace-creator"
        skill_dir.mkdir(parents=True)
        (skill_dir / "SKILL.md").write_text(
            "---\nname: palace-creator\ndescription: test skill\n---\ntest body\n",
            encoding="utf-8",
        )

        palaces = PalaceLoader(str(tmp_path))
        palaces.load_all()
        palace = palaces.get("product-bug")
        assert palace is not None
        assert palace.memory_tags == ["product", "bug"]

        skills = SkillLoader(str(skills_dir))
        skills.load_all()
        for sname in palace.critical_skills:
            assert skills.get(sname) is not None, f"critical skill {sname} not found"
