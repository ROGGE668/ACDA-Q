"""
从策略代码自动提取参数定义，用于前端动态生成参数面板。
兼容 Python 3.11.8
"""
import ast
import re
from typing import List, Dict, Any


class ParamDef:
    """参数定义"""
    def __init__(self, name: str, default: Any, param_type: str):
        self.name = name
        self.default = default
        self.type = param_type  # int / float / str / bool

    def to_dict(self) -> Dict[str, Any]:
        return {
            "name": self.name,
            "default": self.default,
            "type": self.type,
        }


def extract_params(code: str) -> List[Dict[str, Any]]:
    """
    从策略代码中提取 self.params.get(...) 调用，生成参数定义列表。
    """
    params: List[ParamDef] = []
    seen = set()

    try:
        tree = ast.parse(code)
    except SyntaxError:
        return []

    for node in ast.walk(tree):
        if isinstance(node, ast.Call):
            # 匹配 self.params.get("key", default)
            if (
                isinstance(node.func, ast.Attribute)
                and node.func.attr == "get"
                and isinstance(node.func.value, ast.Attribute)
                and node.func.value.attr == "params"
                and isinstance(node.func.value.value, ast.Name)
                and node.func.value.value.id == "self"
            ):
                args = node.args
                if len(args) >= 1 and isinstance(args[0], ast.Constant) and isinstance(args[0].value, str):
                    name = args[0].value
                    default = _parse_default(args[1]) if len(args) > 1 else None
                    param_type = _infer_type(default)
                    if name not in seen:
                        seen.add(name)
                        params.append(ParamDef(name, default, param_type))

    # 兜底：正则匹配 self.params.get("key", default)
    pattern = r'self\.params\.get\(["\'](\w+)["\']\s*,\s*([^)]+)\)'
    for match in re.finditer(pattern, code):
        name = match.group(1)
        if name in seen:
            continue
        default_str = match.group(2).strip()
        default = _parse_str_default(default_str)
        param_type = _infer_type(default)
        seen.add(name)
        params.append(ParamDef(name, default, param_type))

    return [p.to_dict() for p in params]


def _parse_default(node: ast.expr) -> Any:
    """从AST节点解析默认值"""
    if isinstance(node, ast.Constant):
        return node.value
    if isinstance(node, ast.Num):  # Python < 3.8
        return node.n
    if isinstance(node, ast.Str):  # Python < 3.8
        return node.s
    if isinstance(node, ast.NameConstant):  # Python < 3.8
        return node.value
    if isinstance(node, ast.List):
        return [_parse_default(elt) for elt in node.elts]
    if isinstance(node, ast.Dict):
        return {_parse_default(k): _parse_default(v) for k, v in zip(node.keys, node.values)}
    return None


def _parse_str_default(s: str) -> Any:
    """从字符串解析Python字面量"""
    try:
        return ast.literal_eval(s)
    except (ValueError, SyntaxError):
        return s


def _infer_type(value: Any) -> str:
    """根据默认值推断参数类型"""
    if isinstance(value, bool):
        return "bool"
    if isinstance(value, int):
        return "int"
    if isinstance(value, float):
        return "float"
    if isinstance(value, str):
        return "str"
    if isinstance(value, list):
        return "list"
    return "str"
