# ADR-0014: Resolver providers de IA sem identidade fixa

- Status: aceito
- Data: 2026-09-02

## Contexto

O IDE precisa continuar completo quando nenhum provider de IA está instalado.
Vincular os commands de agente ou o frontend ao ID do plugin Codex faria a
plataforma confundir uma implementação opcional com uma dependência do produto e
impediria outros providers de ocupar a mesma fronteira.

## Decisão

O registro seleciona um provider ativo pelo `kind: ai_provider`, nunca por um ID
conhecido pelo núcleo ou pelo frontend. O candidato precisa estar instalado,
habilitado, com a versão atual e com concessões exatamente iguais às permissões
revisadas. Cada operação declara ainda as capabilities e permissões mínimas que
exige antes de alcançar um adapter.

A instalação sem provider é um estado normal e retorna `None` para a interface.
Se houver mais de um provider ativo, a resolução falha fechada enquanto não
existir uma preferência explícita do usuário. Capability ausente, permissão não
declarada ou grant ausente também falham antes de iniciar qualquer processo.

O frontend consulta `ai_provider_current` e recebe somente ID, nome e
capabilities do provider resolvido. Ele não procura o Codex no catálogo e mantém
chat e conta ocultos quando a resposta é vazia ou ambígua. Projeto, Explorer,
editor, Git, terminal, configurações e gerenciador de plugins continuam
independentes dessa consulta.

Adapters são uma camada separada da resolução. Nesta etapa, o módulo builtin
`ai.codex` é o único adapter de conversa implementado e encapsula o Codex App
Server. Um runtime `process` pode declarar capabilities de IA e negociar o
protocolo externo v1, mas o chat não encaminha payloads genéricos: ele retorna
`provider_unsupported` até existir um adapter tipado para streaming, conta,
ferramentas e approvals.

## Consequências

- zero providers não é erro de inicialização nem reduz o IDE;
- o núcleo genérico não contém um ID fixo de provider;
- cada command valida capabilities e grants no momento do uso;
- providers concorrentes exigirão uma escolha persistida antes de serem usados;
- adicionar um provider requer um adapter tipado, sem abrir JSON ou IPC genérico
  entre plugin e frontend;
- detalhes de OpenAI e Codex permanecem restritos ao adapter opcional existente.
