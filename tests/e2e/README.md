# Jornada nativa do MVP

`native.py` abre o binário real com Tauri/WebKitGTK, um repositório temporário e
um perfil XDG separado. O frontend e o IPC de produção permanecem intactos.
Nenhum provider de IA, conta, API key ou download é necessário durante a jornada.
O Git da fixture tem identidade, hooks e assinatura próprios; o repositório de
desenvolvimento e as preferências do usuário não são alterados.

## Execução no Linux

Instale Python 3, Git, Bubblewrap, WebKitWebDriver e as dependências normais de
build. Em Debian/Ubuntu o driver está em `webkit2gtk-driver`. Use uma sessão
gráfica X11/XWayland ou `xvfb-run` com `dbus-run-session`.

```bash
cargo install tauri-driver --version 2.0.5 --locked
npm ci --prefix ui
npm run build --prefix ui
cargo build -p lyrnova --bin lyrnova --example e2e_fixture --locked
python3 tests/e2e/native.py
# Em CI/ambiente sem tela:
xvfb-run -a dbus-run-session -- python3 tests/e2e/native.py
```

`--binary`, `--fixture`, `--driver`, `--native-driver` e `--output` permitem
selecionar caminhos. A fixture externa é empacotada e instalada pelo instalador
Rust de produção; seu estado inicial possui grants explícitos e fica desativado.
A ativação e as Tasks são exercitadas pela interface. O helper recusa destinos
sem o marcador do teste e perfis de plugins já existentes.

O sandbox forte precisa estar disponível: sua ausência **falha** a jornada.
O workflow libera user namespaces apenas na VM efêmera do GitHub Actions,
porque o perfil AppArmor do Ubuntu pode impedir o Bubblewrap. O runner local
não altera sysctls nem políticas do host.

## Cobertura e evidência

- restauração do projeto, ausência de IA e fontes padrão de 16 px;
- edição no Monaco, rascunho sem escrita implícita, salvar e buscar conteúdo;
- diff real, stage, cancelamento da revisão e commit confirmado no Git;
- stdout/stderr, reinício do terminal e limpeza de seus comandos filhos;
- PTY real, cor ANSI renderizada, resize, colagem, Ctrl+C, Ctrl+D e novo shell;
- reinício sob saída contínua, descarte de bytes antigos e recusa de sessão obsoleta;
- plugin externo, revisão de Task, sandbox, streaming e cancelamento de filho;
- falha do plugin com continuidade do editor;
- conflito com edição externa sem perder o rascunho ou sobrescrever o disco;
- reinício com ordem das abas, documento ativo, cursor, rolagem e rascunhos;
- arquivos alterados/excluídos, cópia recuperada e descarte explícito antes de reler;
- recusa de workspace incorreto/token obsoleto, schema futuro preservado e erro visível;
- fechamento com flush, abas intencionalmente fechadas e ausência de replay de comandos.

`target/e2e/report.json` registra as verificações, tempo até o documento aparecer
(desde a solicitação da sessão WebDriver), latência de salvamento e uma amostra
da soma de RSS do driver e seus descendentes. RSS pode contar páginas
compartilhadas mais de uma vez; não é pico, PSS nem medição isolada do aplicativo.
São observações de um build debug e uma fixture pequena, sem limite de aprovação
de desempenho. Falhas também geram screenshot/HTML e `driver.log`.

## Limites da homologação

O WebKit/Wry pode recusar `click`/`sendKeys` nativos, conforme
[tauri-apps/tauri#6541](https://github.com/tauri-apps/tauri/issues/6541).
Por isso o runner aciona handlers DOM, foca inputs e usa edição DOM no Monaco
dentro da janela real. No terminal, aciona colagem e eventos de tecla pelos
handlers do xterm.js. Ele verifica visibilidade/estado dos controles e efeitos
reais no backend, mas **não homologa mouse, teclado do SO, IME ou acessibilidade**.
O modo de entrada é registrado no relatório; não há fallback silencioso.

Abertura/criação via seletor nativo e instalação pelo seletor de pacote ainda
exigem verificação manual: o projeto recente e o pacote são preparados pelo
teste antes da abertura. A recuperação é testada após snapshot confirmado e após fechamento normal. Os
prompts de copiar/descartar usam respostas injetadas no harness DOM. Comandos que deliberadamente criam outra
sessão de processos, crash forçado do host, Wayland nativo, grandes projetos e
outras plataformas continuam fora deste gate. A suíte complementa os testes
Rust de segurança e o smoke visual com IPC sintético em `ui/tests`.

Referência de infraestrutura:
[Tauri WebDriver](https://v2.tauri.app/develop/tests/webdriver/manual-setup/).

O smoke GTK separado (`ui/tests/git-review-smoke.py`) precisa de uma sessão que
renderize quadros (ou display virtual). Tela bloqueada pode suspender os callbacks
de animação; seu relatório inclui `renderedFrame` para diferenciar esse caso.

## Diagnósticos LSP reais

Forneça `--rust-analyzer /caminho/absoluto/rust-analyzer` para incluir o servidor
standalone ELF na jornada. O runner acrescenta um link em uma pasta temporária do
PATH do processo; não instala pacote nem altera o PATH da sessão do usuário. Sem
essa opção, o relatório registra explicitamente que LSP não foi exercitado.
O CI baixa a release 2026-09-21 e confere o SHA-256 antes de executar.

A fixture Cargo verifica cancelamento/revisão de permissões, diagnósticos de um
rascunho, marcadores Monaco, coluna UTF-16 após emoji, navegação e correção. Também
verifica recusa de versões/sessões antigas, reinício, falha real do servidor e
revogação/limpeza ao desativar. O sinal de falha atinge somente o rust-analyzer
identificado entre os descendentes do driver isolado. O arquivo fonte e o marcador
de `build.rs` verificam ausência de escrita/execução de build na fixture, inclusive
com um `rust-analyzer.toml` que tenta habilitá-lo. Isso não certifica toolchains,
dependências externas, biblioteca padrão, projetos grandes ou todas as configurações.
