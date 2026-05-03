"""
安全沙箱执行器：在受限环境中编译和执行用户策略代码。
支持 Python 3.11.8
"""
import ast
import builtins
import sys
import types
import importlib
from typing import Any, Dict, Type

from server.backtest.engine.strategy_base import BaseStrategy

# 允许导入的白名单模块（量化相关）
ALLOWED_MODULES: frozenset[str] = frozenset({
    "__future__",
    "numpy", "np",
    "pandas", "pd",
    "math", "random", "statistics",
    "datetime", "time", "typing",
    "collections", "itertools", "functools",
})

# 禁止使用的名称
BLACKLISTED_NAMES: frozenset[str] = frozenset({
    "__import__", "eval", "exec", "compile",
    "open", "file",
    "getattr", "hasattr", "setattr", "delattr",
    "os", "sys", "subprocess", "socket", "threading", "multiprocessing",
    "requests", "urllib", "http", "ftplib", "smtplib",
    "pathlib", "shutil", "pickle", "marshal",
    "input", "raw_input",
    "exit", "quit",
    "builtins",
})

# 禁止用于反射绕过的 dunder 属性
REFLECTION_DUNDER: frozenset[str] = frozenset({
    "__class__", "__base__", "__bases__", "__mro__",
    "__subclasses__", "__globals__", "__builtins__",
    "__loader__", "__spec__", "__dict__",
})


class SecurityError(Exception):
    """策略代码包含禁止的操作"""
    pass


class StrategyLoadError(Exception):
    """无法从代码中加载策略类"""
    pass


def _validate_ast(tree: ast.AST) -> None:
    """对AST进行静态安全检查"""
    for node in ast.walk(tree):
        # 禁止特定名称的使用
        if isinstance(node, ast.Name):
            if node.id in BLACKLISTED_NAMES:
                raise SecurityError(f"禁止使用名称: {node.id}")
            if node.id in REFLECTION_DUNDER:
                raise SecurityError(f"禁止使用反射属性: {node.id}")

        # 禁止属性访问中的黑名单（如 os.system）
        if isinstance(node, ast.Attribute):
            if node.attr in BLACKLISTED_NAMES:
                raise SecurityError(f"禁止访问属性: {node.attr}")
            if node.attr in REFLECTION_DUNDER:
                raise SecurityError(f"禁止访问反射属性: {node.attr}")

        # 限制导入
        if isinstance(node, ast.Import):
            for alias in node.names:
                top_module = alias.name.split(".")[0]
                if top_module not in ALLOWED_MODULES:
                    raise SecurityError(f"禁止导入模块: {alias.name}")

        if isinstance(node, ast.ImportFrom):
            module = (node.module or "").split(".")[0]
            if module and module not in ALLOWED_MODULES:
                raise SecurityError(f"禁止从模块导入: {node.module}")

        # 禁止函数定义中使用危险装饰器或特定调用
        if isinstance(node, ast.Call):
            if isinstance(node.func, ast.Name):
                if node.func.id in BLACKLISTED_NAMES:
                    raise SecurityError(f"禁止调用函数: {node.func.id}")

        # 禁止 Try 中的 os.exit / sys.exit 等
        if isinstance(node, ast.Raise):
            # 允许 raise，但后续可限制异常类型
            pass


def _safe_import(name, globals=None, locals=None, fromlist=(), level=0):
    """安全的 __import__ 代理：只允许白名单模块"""
    top = name.split(".")[0] if name else ""
    if top and top not in ALLOWED_MODULES:
        raise SecurityError(f"导入被禁止: {name}")
    return builtins.__import__(name, globals, locals, fromlist, level)


def _create_restricted_globals() -> Dict[str, Any]:
    """创建受限的全局命名空间，注入 BaseStrategy 供用户策略继承"""
    restricted_builtins = {
        name: getattr(builtins, name)
        for name in dir(builtins)
        if name not in BLACKLISTED_NAMES
        and not name.startswith("_")
    }
    # class 定义需要 __build_class__
    restricted_builtins["__build_class__"] = builtins.__build_class__
    # 额外移除一些危险内置函数，但用安全代理替换 __import__
    for name in ("open", "eval", "exec", "compile"):
        restricted_builtins.pop(name, None)
    restricted_builtins["__import__"] = _safe_import

    return {
        "__builtins__": restricted_builtins,
        "__name__": "__strategy__",
        "BaseStrategy": BaseStrategy,
    }


def compile_strategy_code(code: str) -> types.ModuleType:
    """
    编译并验证策略代码，返回模块对象。
    若代码通过安全校验，则在受限环境中执行。
    """
    # 1. 语法解析
    try:
        tree = ast.parse(code)
    except SyntaxError as exc:
        raise StrategyLoadError(f"策略代码语法错误: {exc}") from exc

    # 2. AST 安全校验
    _validate_ast(tree)

    # 3. 编译为代码对象
    compiled = compile(tree, filename="<strategy>", mode="exec")

    # 4. 在受限环境中执行
    module = types.ModuleType("__strategy__")
    module.__dict__.update(_create_restricted_globals())

    # 预加载白名单模块到模块命名空间，减少用户import开销
    _preload_allowed_modules(module.__dict__)

    exec(compiled, module.__dict__)
    return module


def _preload_allowed_modules(namespace: Dict[str, Any]) -> None:
    """预加载允许的常用模块到命名空间"""
    preload_map = {
        "numpy": "np",
        "pandas": "pd",
    }
    for module_name, alias in preload_map.items():
        try:
            mod = importlib.import_module(module_name)
            namespace[alias] = mod
            namespace[module_name] = mod
        except Exception:
            pass


def load_strategy_class(module: types.ModuleType) -> Type[BaseStrategy]:
    """
    从已编译的模块中提取 Strategy 类。
    要求类名为 'Strategy' 且继承自 BaseStrategy（或兼容接口）。
    """
    strategy_cls = module.__dict__.get("Strategy")
    if strategy_cls is None:
        raise StrategyLoadError("策略代码中未找到名为 'Strategy' 的类")

    if not isinstance(strategy_cls, type):
        raise StrategyLoadError("'Strategy' 必须是一个类定义")

    # 检查是否有 on_bar 方法（ duck typing ）
    if not callable(getattr(strategy_cls, "on_bar", None)):
        raise StrategyLoadError("策略类必须实现 on_bar(self, context, bar_group) 方法")

    return strategy_cls
