from bender_research import SCHEMA_PACKAGE


def test_schema_package_is_v1() -> None:
    assert SCHEMA_PACKAGE == "bender.v1"
