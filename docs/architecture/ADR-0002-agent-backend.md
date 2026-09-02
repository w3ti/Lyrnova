# ADR-0002: Isolar o App Server no plugin opcional Codex

- Status: revisado; aceito somente para o plugin Codex da issue #37
- Data: 2026-09-01

> Revisão de escopo: o Lyrnova é um IDE, não um cliente de agente. O núcleo,
> a instalação inicial e os fluxos de projeto/editor/Git/terminal não dependem
> do App Server. Os detalhes abaixo definem apenas o adapter do plugin Codex.

## Contexto

Um plugin Codex pode oferecer conversas persistentes, streaming, cancelamento,
approvals e login com conta ChatGPT. Implementar tudo diretamente sobre uma API
exigiria que esse plugin mantivesse também o lifecycle do agente e não
forneceria, por si só, a identidade e o plano ChatGPT do usuário.

O Codex App Server oferece essas superfícies por um protocolo bidirecional
semelhante a JSON-RPC. Em `stdio`, cada mensagem é um frame JSONL. O protocolo
exige `initialize`/`initialized` antes das demais chamadas e disponibiliza
schemas correspondentes à versão do executável.

## Decisão

Adotar uma arquitetura de plugin isolado:

- **núcleo Lyrnova** possui IDE, policy engine, workspace, editor, Git, terminal,
  persistência e protocolo de plugins;
- **plugin Codex** contém o adapter de agente e autenticação por App Server;
- **instalação inicial** não instala, ativa, inicializa nem consulta o Codex;
- **mock** pertence aos testes do plugin, não ao caminho obrigatório do IDE;
- outros providers são plugins separados e não alteram o núcleo.

O código experimental existente permanece temporariamente no workspace durante
a extração da plataforma de plugins. O registro nativo bloqueia todas as suas
operações quando não há um provider compatível instalado e ativo; a seleção usa
tipo, capabilities e grants, sem um ID fixo. O frontend não registra listeners
nem consulta conta nesse estado. A entrega final moverá adapter, processo e
metadados para o pacote do plugin.

A integração inicial usa um processo local e transporte `stdio` JSONL. O modo
WebSocket não faz parte do MVP. O executável compatível será descoberto em
runtime; a política de redistribuição/binário embutido será decidida antes dos
pacotes públicos.

O frontend nunca envia JSON-RPC diretamente. Ele consome somente os tipos
internos do Lyrnova: conta, thread, turn, mensagem, ferramenta, patch, approval,
terminal, uso e erro. O adapter converte entre os dois mundos.

## Autenticação e conta do plugin

O fluxo principal é **Entrar com ChatGPT/OpenAI**:

1. o plugin ativo solicita `account/login/start` ao adapter;
2. App Server devolve uma URL para o navegador ou um código de dispositivo;
3. o usuário autentica no domínio da OpenAI, nunca dentro do webview;
4. após `account/login/completed`, Lyrnova consulta `account/read`;
5. a UI recebe somente modo de autenticação, e-mail opcional e `planType`;
6. logout usa `account/logout` e limpa imediatamente o estado de conta da UI.

Tokens, cookies, senha e respostas brutas de autenticação não atravessam a
fronteira do adapter. Login por API key poderá existir como opção avançada, mas
não será implementado antes de um secret store nativo.

## Fronteiras de responsabilidade

| Responsabilidade | Núcleo Lyrnova | Plugin Codex/App Server |
| --- | --- | --- |
| IDE, projetos, editor, Git e terminal | sim | não |
| Interface e estado visual do chat | hospeda superfície mediada | fornece dados |
| Conta ChatGPT, sessão e refresh | não | sim |
| E-mail/plano exibidos | projeção mínima | fonte |
| Threads, turns e streaming do agente | contrato e isolamento | executa |
| Decisão final de autoridade local | sim | não |
| Sandbox e approvals | política adicional obrigatória | sinais e política do agente |
| Git, arquivos e processos expostos pela aplicação | sim | não diretamente |

Mesmo que o backend solicite ou reporte uma ação, isso não concede autoridade
ao aplicativo. Toda operação local oferecida pelo Lyrnova continua sujeita à
ADR-0003.

## Compatibilidade e falha segura

- `clientInfo.name` será `lyrnova` e a versão será a versão do aplicativo;
- a conexão falha se o handshake ou a forma mínima das mensagens for inválida;
- frames JSONL são limitados antes do parse;
- requests pendentes são vinculados a IDs e ao método esperado;
- campos desconhecidos podem ser ignorados, mas campos obrigatórios inválidos
  não recebem fallback permissivo;
- API experimental fica desligada por padrão;
- incompatibilidade nunca libera ferramenta, comando ou escrita;
- logs não incluem credenciais, e-mail ou conteúdo de conversa.

## Evidência inicial

- `codex-cli 0.152.0` instalado localmente expõe `codex app-server` e o
  transporte `stdio://`;
- `backend.rs` cobre handshake, framing, correlação de requests, conta,
  browser login, device-code login, logout e erros remotos;
- fixtures garantem que um token presente numa resposta bruta não aparece no
  `AccountSummary` entregue à UI;
- o reducer independente já cobre streaming idempotente, approvals e falhas de
  turn.

O transporte de processo, a consulta de conta e o teste local opt-in estão
implementados como base experimental do plugin. Login, logout,
criação/retomada de thread, início de turn e deltas de mensagem também
atravessam o adapter. Turns reais usam `readOnly` e
`approvalPolicy: on-request`. O broker aceita somente requests de comando e
alteração de arquivo reconhecidos, cria uma capability de uso único vinculada à
thread, ao turn e ao item, e encaminha ao App Server apenas a decisão explícita
recebida da interface. Métodos ou vínculos inesperados falham fechados.

## Alternativas rejeitadas agora

### Apenas Responses API

Preserva máximo controle, mas amplia muito o escopo do primeiro MVP e não cobre
o login/planos ChatGPT desejado. Continua viável como adapter futuro.

### UI acoplada ao App Server

Reduz código inicial, mas torna o frontend dependente de versões e tipos do
provider e enfraquece testes e a fronteira de segurança.

### Copiar uma implementação existente

Contraria a identidade e a manutenção próprias do projeto. Lyrnova implementa
seu cliente e seus contratos; dependências externas permanecem explícitas,
versionadas e auditáveis.

## Referências oficiais

- [Codex App Server](https://learn.chatgpt.com/docs/app-server)
- [Agent approvals & security](https://learn.chatgpt.com/docs/agent-approvals-security)
