import { defineConfig } from "vitepress";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const __dirname = dirname(fileURLToPath(import.meta.url));

// Load the 1y TextMate grammar for Shiki syntax highlighting in code blocks.
// This is the same grammar file used by the VSCode extension
// (editor/syntaxes/1y.tmLanguage.json), ensuring consistent highlighting
// between the editor and the docs.
const grammar = JSON.parse(
    readFileSync(resolve(__dirname, "../../editor/syntaxes/1y.tmLanguage.json"), "utf-8")
);

// Shared theme configuration — nav + sidebar per locale.
const zhNav = [
    { text: "设计哲学", link: "/zh/philosophy/design-principles" },
    { text: "语法基础", link: "/zh/syntax/getting-started" },
    { text: "参考例子", link: "/zh/examples/hello-world" },
];

const enNav = [
    { text: "Philosophy", link: "/en/philosophy/design-principles" },
    { text: "Syntax", link: "/en/syntax/getting-started" },
    { text: "Examples", link: "/en/examples/hello-world" },
];

const zhSidebar = {
    "/zh/philosophy/": [
        {
            text: "设计哲学",
            items: [
                { text: "设计原则", link: "/zh/philosophy/design-principles" },
                { text: "数值统一", link: "/zh/philosophy/numerical-unification" },
                { text: "并发模型", link: "/zh/philosophy/concurrency-model" },
                { text: "为什么不实现 async/await", link: "/zh/philosophy/no-async" },
                { text: "字节码虚拟机", link: "/zh/philosophy/bytecode-vm" },
            ],
        },
    ],
    "/zh/syntax/": [
        {
            text: "入门",
            items: [
                { text: "快速开始", link: "/zh/syntax/getting-started" },
                { text: "词法结构", link: "/zh/syntax/lexical-structure" },
            ],
        },
        {
            text: "类型与表达式",
            items: [
                { text: "类型系统", link: "/zh/syntax/types" },
                { text: "表达式与运算符", link: "/zh/syntax/expressions" },
                { text: "语句", link: "/zh/syntax/statements" },
            ],
        },
        {
            text: "核心特性",
            items: [
                { text: "模式匹配", link: "/zh/syntax/pattern-matching" },
                { text: "函数与闭包", link: "/zh/syntax/functions" },
                { text: "自定义类型", link: "/zh/syntax/custom-types" },
                { text: "控制流", link: "/zh/syntax/control-flow" },
            ],
        },
        {
            text: "模块与并发",
            items: [
                { text: "模块系统", link: "/zh/syntax/modules" },
                { text: "Actor 模型", link: "/zh/syntax/actors" },
                { text: "事务内存 (STM)", link: "/zh/syntax/stm" },
                { text: "无色异步", link: "/zh/syntax/async" },
                { text: "多线程", link: "/zh/syntax/multithreading" },
                { text: "标准库概览", link: "/zh/syntax/stdlib" },
                { text: "FFI 外部函数接口", link: "/zh/syntax/ffi" },
                { text: "反射与动态求值", link: "/zh/syntax/introspection" },
            ],
        },
    ],
    "/zh/examples/": [
        {
            text: "参考例子",
            items: [
                { text: "Hello World", link: "/zh/examples/hello-world" },
                { text: "斐波那契数列", link: "/zh/examples/fibonacci" },
                { text: "Actor 键值存储", link: "/zh/examples/actor-kv" },
                { text: "事务性计数器", link: "/zh/examples/transactional-counter" },
                { text: "TLS HTTP 客户端", link: "/zh/examples/http-client" },
                { text: "yin Web 框架", link: "/zh/examples/yin-server" },
                { text: "STM 银行转账", link: "/zh/examples/stm-bank" },
                { text: "并发 Web 爬虫", link: "/zh/examples/concurrent-crawler" },
                { text: "性能基准测试", link: "/zh/examples/benchmark" },
            ],
        },
    ],
};

const enSidebar = {
    "/en/philosophy/": [
        {
            text: "Philosophy",
            items: [
                { text: "Design Principles", link: "/en/philosophy/design-principles" },
                { text: "Numerical Unification", link: "/en/philosophy/numerical-unification" },
                { text: "Concurrency Model", link: "/en/philosophy/concurrency-model" },
                { text: "Why No async/await", link: "/en/philosophy/no-async" },
                { text: "Bytecode VM", link: "/en/philosophy/bytecode-vm" },
            ],
        },
    ],
    "/en/syntax/": [
        {
            text: "Getting Started",
            items: [
                { text: "Quick Start", link: "/en/syntax/getting-started" },
                { text: "Lexical Structure", link: "/en/syntax/lexical-structure" },
            ],
        },
        {
            text: "Types & Expressions",
            items: [
                { text: "Type System", link: "/en/syntax/types" },
                { text: "Expressions & Operators", link: "/en/syntax/expressions" },
                { text: "Statements", link: "/en/syntax/statements" },
            ],
        },
        {
            text: "Core Features",
            items: [
                { text: "Pattern Matching", link: "/en/syntax/pattern-matching" },
                { text: "Functions & Closures", link: "/en/syntax/functions" },
                { text: "Custom Types", link: "/en/syntax/custom-types" },
                { text: "Control Flow", link: "/en/syntax/control-flow" },
            ],
        },
        {
            text: "Modules & Concurrency",
            items: [
                { text: "Module System", link: "/en/syntax/modules" },
                { text: "Actor Model", link: "/en/syntax/actors" },
                { text: "Software Transactional Memory", link: "/en/syntax/stm" },
                { text: "Colorless Async", link: "/en/syntax/async" },
                { text: "Multi-threading", link: "/en/syntax/multithreading" },
                { text: "Standard Library", link: "/en/syntax/stdlib" },
                { text: "FFI", link: "/en/syntax/ffi" },
                { text: "Reflection & Dynamic Evaluation", link: "/en/syntax/introspection" },
            ],
        },
    ],
    "/en/examples/": [
        {
            text: "Examples",
            items: [
                { text: "Hello World", link: "/en/examples/hello-world" },
                { text: "Fibonacci", link: "/en/examples/fibonacci" },
                { text: "Actor KV Store", link: "/en/examples/actor-kv" },
                { text: "Transactional Counter", link: "/en/examples/transactional-counter" },
                { text: "TLS HTTP Client", link: "/en/examples/http-client" },
                { text: "yin Web Framework", link: "/en/examples/yin-server" },
                { text: "STM Bank Transfers", link: "/en/examples/stm-bank" },
                { text: "Concurrent Web Crawler", link: "/en/examples/concurrent-crawler" },
                { text: "Performance Benchmark", link: "/en/examples/benchmark" },
            ],
        },
    ],
};

export default defineConfig({
    title: "1y",
    description: "1y 编程语言官方文档 — The 1y Programming Language",

    // GitHub Pages serves at https://okysu.github.io/1y/
    base: "/1y/",

    // Clean URL without trailing .html
    cleanUrls: true,

    // Syntax highlighting: register our custom 1y TextMate grammar (the same
    // file used by the VSCode extension) plus common languages for docs.
    markdown: {
        lineNumbers: true,
        languages: [
            "typescript",
            "javascript",
            "rust",
            "bash",
            "json",
            "yaml",
            "toml",
            // Custom 1y language — the grammar object is a valid
            // LanguageRegistration (it extends IRawGrammar with name/scopeName).
            {
                ...grammar,
                aliases: ["onely"],
            },
        ],
    },

    locales: {
        en: {
            label: "English",
            lang: "en",
            themeConfig: {
                nav: enNav,
                sidebar: enSidebar,
                outline: { label: "On this page" },
                docFooter: { prev: "Previous", next: "Next" },
                lastUpdated: { text: "Last updated" },
                returnToTopLabel: "Back to top",
                sidebarMenuLabel: "Menu",
                darkModeSwitchLabel: "Appearance",
            },
        },
        zh: {
            label: "中文",
            lang: "zh-CN",
            themeConfig: {
                nav: zhNav,
                sidebar: zhSidebar,
                outline: { label: "本页目录" },
                docFooter: { prev: "上一页", next: "下一页" },
                lastUpdated: { text: "最后更新" },
                returnToTopLabel: "回到顶部",
                sidebarMenuLabel: "菜单",
                darkModeSwitchLabel: "外观",
            },
        },
    },

    themeConfig: {
        socialLinks: [
            { icon: "github", link: "https://github.com/Okysu/1y" },
        ],
        search: {
            provider: "local",
            options: {
                translations: {
                    button: {
                        buttonText: "Search",
                        buttonAriaLabel: "Search",
                    },
                    modal: {
                        noResultsText: "No results",
                        resetButtonTitle: "Reset",
                        footer: {
                            selectText: "Select",
                            navigateText: "Navigate",
                        },
                    },
                },
            },
        },
    },
});
