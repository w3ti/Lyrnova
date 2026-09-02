# Protocolo de runtimes externos

Status: contrato inicial do protocolo v1.

O entrypoint de um plugin `process` recebe e envia um objeto JSON por linha em
UTF-8. Stdout é reservado ao protocolo. Cada frame deve terminar com `\n` e ter
no máximo 256 KiB. O processo roda dentro do sandbox e não deve tratar stdin
como terminal.

## Inicialização

O host sempre envia primeiro:

```json
{"type":"initialize","protocol_version":1,"plugin_id":"io.github.example.tool","plugin_version":"1.0.0","capabilities":["tasks"],"permissions":["workspace_read","process_spawn"]}
```

O plugin deve responder em até três segundos, repetindo exatamente as
capabilities declaradas:

```json
{"type":"ready","protocol_version":1,"capabilities":["tasks"]}
```

Versão incompatível, capabilities ausentes/adicionais, outro tipo de frame ou
qualquer campo desconhecido encerram a sessão.

## Request, response e eventos

Uma chamada do host contém ID, capability, operação namespaced e payload:

```json
{"type":"request","request_id":"8f39d6d2d8474a49a3ba7f84a24590f0","capability":"tasks","operation":"tasks.list","payload":{}}
```

O plugin responde com o mesmo ID e capability:

```json
{"type":"response","request_id":"8f39d6d2d8474a49a3ba7f84a24590f0","capability":"tasks","result":{"items":[]}}
```

Uma falha funcional não quebra a sessão:

```json
{"type":"error","request_id":"8f39d6d2d8474a49a3ba7f84a24590f0","capability":"tasks","code":"not_configured","message":"Nenhuma task foi configurada."}
```

Eventos assíncronos também permanecem vinculados à capability:

```json
{"type":"event","capability":"tasks","event":"task.output","payload":{"chunk":"Compilando…"}}
```

O host envia `{"type":"shutdown"}` antes do encerramento normal. Um runtime não
deve depender da janela de shutdown para preservar dados importantes.

## Limites de segurança

- IDs, operações, eventos e códigos usam somente ASCII alfanumérico, `.`, `_`,
  `-` e `:` e têm até 128 bytes;
- a operação começa com o nome exato da capability e `.`, por exemplo
  `syntax_highlighting.tokens`;
- floats, NUL, profundidade acima de 16, coleções acima de 4.096 itens e strings
  acima de 128 KiB são inválidos;
- o runtime não inicia se o handshake não corresponder ao manifesto;
- respostas fora de ordem, IDs divergentes, eventos de outra capability, EOF,
  timeout ou excesso de eventos encerram a sessão;
- payloads não concedem autoridade e nunca são encaminhados diretamente ao
  frontend nem interpretados como comandos.
