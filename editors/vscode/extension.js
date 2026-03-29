const vscode = require('vscode');
const { LanguageClient, TransportKind } = require('vscode-languageclient/node');

let client;

function activate(context) {
    const serverOptions = {
        command: 'tryzub',
        args: ['lsp'],
        transport: TransportKind.stdio
    };

    const clientOptions = {
        documentSelector: [{ scheme: 'file', language: 'tryzub' }],
    };

    client = new LanguageClient('tryzub-lsp', 'Тризуб LSP', serverOptions, clientOptions);
    client.start();
}

function deactivate() {
    if (client) return client.stop();
}

module.exports = { activate, deactivate };
