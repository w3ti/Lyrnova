# ADR-0001: Rust e Tauri 2 para um workspace totalmente local

- Status: aceito
- Data: 2026-09-01

## Contexto

O primeiro protótipo foi concebido como wrapper de uma página remota. O produto
foi redefinido como ambiente de desenvolvimento com projetos, agente, diff,
terminal e approvals. Isso exige uma interface local e um núcleo com autoridade
explícita sobre filesystem e processos.

## Decisão

Manteremos Rust 2024 e Tauri 2. O frontend será empacotado com o aplicativo e
não carregará scripts, estilos, fontes ou frames remotos. O WebKitGTK no Linux e
o WebView2 no Windows renderizam apenas a interface local.

O núcleo Rust será dividido em protocolo, adapters de agente, workspace,
persistência, ferramentas, processos, Git e políticas. O frontend nunca
executará shell ou filesystem diretamente: commands Tauri serão estreitos,
tipados e vinculados a capabilities.

## Consequências

- o aplicativo abre e funciona em modo mock sem rede ou credenciais;
- CSP pode negar conexões de rede ao renderer;
- todo efeito local passa pelo núcleo Rust;
- diferenças Linux/Windows ficam atrás de interfaces;
- o adapter real pode mudar sem reescrever a UI;
- Electron não faz parte da arquitetura atual.

## Verificação

- configuração Tauri contém somente janelas locais;
- frontend não referencia recursos HTTP;
- CI testa CSP, capabilities e protocolo;
- mocks reproduzem streaming, patches, approvals e erros.
