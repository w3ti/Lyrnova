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

Em `native.py`, o projeto recente e o pacote são preparados antes da abertura.
A suíte complementar `native_input.py`, descrita abaixo, exercita os seletores
nativos e a entrada do SO em uma tela isolada. A recuperação é testada após snapshot confirmado e após fechamento normal. Os
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

A análise requer Cargo e rustc da distribuição em `/usr/bin`, visíveis no sandbox.
A jornada Rust também cobre hover pelo Monaco (F1 → **Show or Focus Hover**), conteúdo inerte
e F12 abrindo a definição em uma aba ainda não carregada. Testes de concorrência
e cancelamento dos providers podem ser executados com `npm test --prefix ui`.

A jornada de ambiente usa Cargo/rustc reais copiados para uma estrutura temporária
de toolchain e registry isolado com crate de teste. Ela revisa a seleção pelo
produto e exercita F12 externo sem escrita em fonte/lockfile. `--rust-src` pode
apontar para o diretório `library` de uma instalação rust-src compatível para
validar a biblioteca padrão; o CI usa o pacote da distribuição. Exemplo local:
`--rust-src /usr/lib/rustlib/src/rust/library`. A ausência dessa opção não homologa
resolução de std/core. A evidência local desta fase fica em `target/e2e-environment`.

## Ações de edição Rust

A fixture também precisa de `/usr/bin/rustfmt`; o runner copia o binário real para
a toolchain temporária. O gate testa Ctrl+Espaço e aceitação de completion,
Shift+F12 com navegação, F2 alterando chamada e definição num arquivo antes fechado,
e Shift+Alt+F com formatação real. Verifica ausência de gravação em disco e desfazer
por arquivo. Há testes unitários adicionais para sobreposição/UTF-16, versões,
operações proibidas, cancelamento e preparação integral dos rascunhos. Evidência
local: `target/e2e-rust-actions/report.json`.

## Mudanças externas no projeto

A jornada cria, substitui, renomeia e exclui arquivos pelo filesystem, sem acionar
refresh no produto. Verifica abas limpas ativas/inativas, preservação de rascunhos,
conflito antes de salvar, reload explícito, recriação, substituição binária e save
próprio sem falso conflito. Requests de polling/leitura para outro workspace são
recusados. Com Rust, uma fonte fechada muda de tipo e linha; a análise reinicia,
hover/definição refletem o novo disco e o rascunho do chamador permanece intacto.
A evidência local fica em `target/e2e-watch/report.json`.

Se a combinação local de tauri-driver/WebKitWebDriver encerrar conexões no proxy,
`--direct-native` mantém a criação de sessões e o lifecycle no tauri-driver, mas
envia comandos da sessão diretamente à porta nativa. O relatório registra
`commandTransport`; não há retry implícito de clicks/comandos que poderiam ter
sido executados antes de perder a resposta. Esse modo exercita o mesmo binário,
WebView e IPC reais. O CI também usa o modo direto desde `e7fed1b`, após a mesma desconexão ocorrer
no runner Ubuntu. Nenhuma ação é repetida implicitamente ao perder uma resposta.

A jornada Rust também verifica auto-import via Ctrl+Espaço (símbolo e `use` em
um único desfazer), resolução por ID opaco e correção rápida via Ctrl+.
O arquivo em disco precisa permanecer inalterado após ambas as ações; apenas
os rascunhos recebem as edições. Atalhos continuam acionados pelos handlers DOM.


## Monitor incremental e projetos maiores

A jornada cria uma árvore de 6 mil arquivos fora do projeto e a move para dentro.
Verifica observação por eventos, listagem completa, recarga de aba limpa sem
reconstruir a árvore do Explorer, preservação de rascunho, rename e remoção da pasta.
O cenário é removido antes de continuar os testes Rust. Os testes de backend
usam 12 mil arquivos e contadores de inspeção, além de overflow e quota de watches.
Evidência local: `target/e2e-incremental/report.json`.

## Teclado, mouse e seletores GTK

`native_input.py` usa XTest (`libX11` e `libXtst`) para enviar teclas e cliques ao
servidor X11. WebDriver somente lê estado e geometria do DOM; não aciona handlers,
injeta texto, responde prompts ou chama IPC. A execução começa sem projeto
recente nem plugin instalado no perfil da aplicação. O pacote de teste é gerado
em outro perfil e selecionado pelo diálogo real, junto do descritor SHA-256.
Favoritos GTK apontam apenas às pastas da fixture; a seleção das pastas usa
Alt+1…4 e Enter no diálogo real, e a seleção do pacote usa seu campo de localização.

```bash
# Dependências adicionais no Debian/Ubuntu:
# xvfb libxtst6 dbus-x11 python3-gi gir1.2-gtk-3.0
python3 tests/e2e/native_input.py --xvfb /usr/bin/Xvfb
# Escala inteira GTK de 200%, com tela física ampliada:
python3 tests/e2e/native_input.py --xvfb /usr/bin/Xvfb --scale 2 --output target/e2e-input-2x
# Alternativa quando o chamador já possui uma tela descartável:
xvfb-run -a -s '-screen 0 1440x1000x24' dbus-run-session -- \
  python3 tests/e2e/native_input.py --isolated-display
```

`--xvfb` inicia um display livre sem escuta TCP, um D-Bus privado e diretórios
temporários para configurações, dados e cache. Encerra apenas os processos do
teste. `--isolated-display` exige que o chamador forneça uma tela descartável:
não use essa opção na sua sessão de trabalho. Os caminhos do binário, fixture,
drivers e evidências aceitam as mesmas opções do outro runner; o transporte dos
comandos WebDriver é direto ao driver nativo.

A jornada cobre abertura/cancelamento de pasta, criação por Tab/Enter, cancelamento
e nova tentativa, Git inicial, fechamento do modal, edição/salvamento/desfazer,
paleta, terminal com Ctrl+C/Ctrl+D, troca de projeto com rascunho e revisão de
instalação de plugin. Cada seletor também exige que o WebView continue respondendo.
O relatório fica em `target/e2e-input/report.json`, com screenshot/HTML em falhas.
Se ImageMagick estiver disponível, a falha também captura a tela com o diálogo GTK.

O gate também envia teclas mortas (agudo, til e circunflexo) e cedilha pelo XTest,
confere os bytes UTF-8 salvos e troca texto com um editor GTK em outro processo.
`clipboard_peer.py` usa `/usr/bin/python3` e PyGObject da distribuição; copiar e
colar passam pelos atalhos normais do GTK. O texto inclui acentos, emoji, grego,
japonês e quebras de linha. A presença de japonês no clipboard não testa um IME.
No terminal, Ctrl+Shift+V cola sem executar até Enter e Ctrl+Shift+C copia uma
linha selecionada por mouse para o editor externo; Ctrl+C ainda interrompe jobs.

Tab/Shift+Tab devem ficar dentro da paleta, com foco visível; Ctrl+K repetido e
Escape restauram o editor. Comandos da paleta transferem foco ao novo diálogo,
atalhos globais não escapam desse modal e fechar o terminal focado restaura o
editor. O mapa de quatro teclas, clipboard e perfis pertencem à tela descartável.

Esse gate cobre X11/Xvfb e escalas inteiras GTK (`--scale 1` ou `2`), não escala
fracionária de compositor. Não substitui homologação humana, IBus/Fcitx e seleção
de candidatos, leitor de tela, arrastar e soltar ou Wayland nativo. Consulte a
[matriz de desktop](../../docs/product/desktop-validation-2026-09-27.md) para
os aceites restantes. A suíte DOM continua responsável pela matriz mais ampla
de Git, Tasks, recuperação e linguagem Rust.
