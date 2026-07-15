// 1y VSCode extension client — starts the language server, registers commands,
// wires up configuration, and (optionally) installs inline-suggestion decorations.
//
// The server is the bundled `dist/server.js` (esbuild output of src/server.ts).
// It runs as a child process over stdio. We also expose two commands:
//   - `onely.showOutput`    : reveal the LSP output channel
//   - `onely.restartServer` : kill + restart the server (useful while developing)

import {
    ExtensionContext,
    OutputChannel,
    commands,
    workspace,
    window,
    Disposable,
} from "vscode";

import {
    LanguageClient,
    LanguageClientOptions,
    ServerOptions,
    TransportKind,
    State,
} from "vscode-languageclient/node";

import { InlineSuggestionProvider } from "./inline-suggestions";

let client: LanguageClient | null = null;
let outputChannel: OutputChannel | null = null;
let inlineProvider: InlineSuggestionProvider | null = null;
let disposables: Disposable[] = [];

export async function activate(context: ExtensionContext): Promise<void> {
    outputChannel = window.createOutputChannel("1y");
    context.subscriptions.push(outputChannel);

    // ----- Start the language server -------------------------------------
    startServer(context);

    // ----- Commands ------------------------------------------------------
    context.subscriptions.push(
        commands.registerCommand("onely.showOutput", () => {
            outputChannel?.show();
        }),
        commands.registerCommand("onely.restartServer", async () => {
            outputChannel?.appendLine("[client] restarting language server...");
            await stopServer();
            startServer(context);
            outputChannel?.appendLine("[client] language server restarted");
        }),
    );

    // ----- Inline suggestions -------------------------------------------
    const inlineEnabled = () =>
        workspace.getConfiguration("1y").get<boolean>("inlineSuggestions.enabled", true);

    if (inlineEnabled()) {
        inlineProvider = new InlineSuggestionProvider(context);
        inlineProvider.activate();
    }

    // Watch for toggling inline suggestions at runtime.
    context.subscriptions.push(
        workspace.onDidChangeConfiguration((e) => {
            if (e.affectsConfiguration("1y.inlineSuggestions.enabled")) {
                if (inlineEnabled() && !inlineProvider) {
                    inlineProvider = new InlineSuggestionProvider(context);
                    inlineProvider.activate();
                } else if (!inlineEnabled() && inlineProvider) {
                    inlineProvider.dispose();
                    inlineProvider = null;
                }
            }
        }),
    );

    outputChannel.appendLine("[client] 1y extension activated");
}

export async function deactivate(): Promise<void> {
    for (const d of disposables) {
        try { d.dispose(); } catch { /* ignore */ }
    }
    disposables = [];
    if (inlineProvider) {
        inlineProvider.dispose();
        inlineProvider = null;
    }
    await stopServer();
}

// ---------------------------------------------------------------------------
// Server lifecycle
// ---------------------------------------------------------------------------

function resolveServerModule(context: ExtensionContext): string {
    // The server is bundled as dist/server.js by esbuild.
    return context.asAbsolutePath("dist/server.js");
}

function startServer(context: ExtensionContext): void {
    const serverModule = resolveServerModule(context);

    const serverOptions: ServerOptions = {
        run: {
            module: serverModule,
            transport: TransportKind.stdio,
        },
        debug: {
            module: serverModule,
            transport: TransportKind.stdio,
            options: { execArgv: ["--nolazy", "--inspect=6009"] },
        },
    };

    const executablePath = workspace.getConfiguration("1y").get<string>("executablePath", "");

    const clientOptions: LanguageClientOptions = {
        documentSelector: [{ scheme: "file", language: "1y" }],
        synchronize: {
            configurationSection: "1y",
            fileEvents: workspace.createFileSystemWatcher("**/*.1y"),
        },
        outputChannel: outputChannel || undefined,
        traceOutputChannel: outputChannel || undefined,
        initializationOptions: {
            executablePath,
        },
    };

    client = new LanguageClient(
        "1y-language-server",
        "1y Language Server",
        serverOptions,
        clientOptions,
    );

    // Hook state changes into the output channel for debugging.
    client.onDidChangeState((e) => {
        const stateName = e.newState === State.Running ? "Running"
            : e.newState === State.Starting ? "Starting"
            : "Stopped";
        outputChannel?.appendLine(`[client] server state: ${stateName}`);
    });

    // The client itself implements Disposable; start() returns a Promise.
    disposables.push(client);
    client.start().catch((e) => {
        outputChannel?.appendLine(`[client] failed to start server: ${e}`);
    });
}

async function stopServer(): Promise<void> {
    if (client) {
        try {
            await client.stop();
        } catch (e) {
            outputChannel?.appendLine(`[client] error stopping server: ${e}`);
        }
        client = null;
    }
}
