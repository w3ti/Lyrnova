<p align="center">
  <img src="assets/brand/lyrnova-logo-original.png" alt="Lyrnova" width="760">
</p>

<p align="center">
  IDE desktop multiplataforma, extensível e seguro, construído com Rust e Tauri.
</p>

<p align="center">
  <strong>Em desenvolvimento inicial.</strong>
</p>

## Sobre

Lyrnova é um IDE desktop comunitário. Ele reúne projetos, Explorer, edição
inteligente, Git, diff, terminal, tasks, testes e depuração em uma interface
própria. Linguagens, runtimes, frameworks e provedores de IA são plugins
instalados e ativados pelo usuário.

O núcleo não depende de Codex, OpenAI ou outro serviço de IA. A instalação
inicial não inclui o Codex e o IDE continua completamente utilizável sem conta,
API key ou provider remoto.

## Tecnologia

- Rust como núcleo nativo;
- Tauri 2 para a interface local e integrações de desktop;
- Monaco Editor empacotado localmente para edição, cores e autocomplete;
- WebKitGTK no Linux e WebView2 no Windows;
- plataforma de plugins para linguagens, LSP, DAP, templates, tasks e IA;
- processos de plugins isolados e APIs mediadas pelo núcleo;
- permissões e approvals antes de efeitos sensíveis;
- frontend empacotado, sem recursos web remotos.

## Plugins

O catálogo curado e os releases de plugins são derivados do GitHub. Metadados
remotos têm versão, expiração e assinaturas raiz; cada release é autenticada por
uma chave Ed25519 delegada ao publisher e verificada também por SHA-256. Instalação
local, download, ativação, desativação e desinstalação passam pelas fronteiras Rust
do gerenciador do Lyrnova.

Plugins de linguagem poderão fornecer syntax highlighting, tokens semânticos,
LSP/autocomplete, DAP/debug, templates, tasks, testes e toolchains. Plugins de
IA poderão fornecer autenticação, modelos, chat, ferramentas e approvals sem
entrar no núcleo do IDE.

Runtimes externos negociam um protocolo JSONL versionado e só recebem chamadas
namespaced pelas capabilities declaradas no manifesto. IDs são gerados pelo host,
frames e payloads são limitados e nenhuma mensagem vira shell ou IPC genérico do
frontend.

Providers de IA ativos são resolvidos por tipo, capabilities, permissões e grants,
sem um ID fixo no núcleo ou no frontend. Nenhum provider é um estado normal; se
mais de um estiver ativo, o uso falha fechado até existir uma seleção explícita.

Primeira coleção planejada:

- linguagens e runtimes: Rust, Angular, React, Node.js e C/C++;
- provedores de IA opcionais: Codex e Gemini.

O catálogo embarcado permanece vazio até existir uma release externa revisada;
nenhum plugin fictício é oferecido para download. No Linux, runtimes externos
são iniciados com Bubblewrap e recebem somente os mounts e a rede concedidos.

## Plataformas planejadas

- Windows;
- Fedora;
- Debian;
- Ubuntu;
- openSUSE;
- Lyra OS;
- OpenBase.

Cada plataforma receberá um pacote próprio e usará, sempre que possível, seu
gerenciador nativo para instalação e atualizações.

## Estado do projeto

O projeto está na fase de fundação funcional. O workspace Tauri 2 já oferece
criação e abertura nativa de projetos, Explorer, Monaco Editor, abas, syntax e
autocomplete, operações Git locais, terminal e painéis redimensionáveis. A
plataforma de plugins já cobre manifesto, instalação transacional, catálogo,
remoção, lifecycle sandboxed e protocolo funcional limitado por capability.

A integração experimental com Codex App Server é acessada pelo adapter do plugin
opcional correspondente. Ela não representa uma dependência nem o propósito
central do produto e permanece ausente da instalação inicial.

## Arquitetura

O frontend local apresenta projetos, editor, Explorer, Git/diff, gerenciador de
plugins e terminal. O núcleo Rust é responsável por workspaces, documentos,
persistência, processos, permissões e Git. Plugins pedem capabilities pelas
APIs do Lyrnova; somente o núcleo concede autoridade e executa efeitos.

- [Escopo e não objetivos](docs/product/scope.md)
- [Decisões arquiteturais](docs/architecture/)
- [Contrato de manifesto de plugins](docs/plugins/manifest.md)
- [Threat model](docs/security/threat-model.md)
- [Design system](docs/design-system.md)

## Desenvolvimento local

No Linux, são necessários Rust stable, Node.js 24, GTK 3 e WebKitGTK 4.1.
Bubblewrap também é obrigatório para ativar plugins externos. Com as dependências
do sistema instaladas:

```bash
npm ci --prefix ui
npm run build --prefix ui
cargo test --workspace
cargo run -p lyrnova
```

Os testes normais são locais e não precisam de conta, API key ou rede. Testes
de providers são opcionais, isolados e nunca requisito para validar o IDE.

Consulte [CONTRIBUTING.md](CONTRIBUTING.md) antes de alterar capabilities,
protocolo, filesystem, processos ou integrações nativas.

## Independência

Lyrnova é um projeto comunitário independente. Não é produzido, endossado nem
suportado pela OpenAI, Google ou pelos mantenedores das tecnologias disponíveis
como plugins. As marcas citadas pertencem aos respectivos titulares.

## Licença

O código autoral do Lyrnova é distribuído exclusivamente sob a GNU General
Public License versão 3 (`GPL-3.0-only`). Dependências e componentes de
terceiros permanecem sob suas respectivas licenças.
