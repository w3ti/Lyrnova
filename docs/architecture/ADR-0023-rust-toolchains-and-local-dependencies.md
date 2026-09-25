# ADR-0023 — Toolchains e dependências Rust locais

- Data: 2026-09-25
- Estado: implementação local; evidência no checkpoint

## Escopo

O painel Problemas oferece **Ambiente Rust** para escolher ferramentas já
instaladas e revisar a leitura de fontes externas. A descoberta não executa rustup
nem instala componentes: considera `/usr` e os diretórios de toolchains em
`RUSTUP_HOME/toolchains`, ou `~/.rustup/toolchains`. Cargo e rustc precisam existir
como executáveis dentro da toolchain; symlinks para o launcher rustup são recusados.
Toolchains dentro do workspace não entram na descoberta.

O sistema é a opção inicial. A biblioteca padrão é habilitada quando suas fontes
estão instaladas dentro da toolchain; sem rust-src, a interface informa a limitação.
Arquivos rust-toolchain.toml, overrides e defaults do rustup não escolhem a versão
automaticamente. A seleção explícita usa os binários reais no sandbox. Paths de
ferramentas personalizados, instalação/upgrade de toolchains e build/test permanecem
fora desta entrega.

## Revisão e autorização

A revisão mostra a localização da toolchain e da biblioteca padrão, além dos
subdiretórios `registry/index`, `registry/cache` e `registry/src` disponíveis no
Cargo home. O uso de registry começa desmarcado; desativar dependências também
remove essa seleção. Não são montados config.toml, credentials.toml, Cargo bin,
git databases/checkouts ou o HOME do usuário. Crates privadas presentes nos
subdiretórios aprovados fazem parte da leitura concedida.

O backend emite token efêmero de revisão, vinculado ao workspace, com validade de
dez minutos e uso único. Aplicar compara a seleção com a descoberta atual; não
aceita paths arbitrários do frontend. Todas as operações revalidam o plugin Rust,
seus grants e o workspace sob o lock de lifecycle. A revisão é adicional e
específica desse consumidor; não aumenta permissões genéricas de plugins externos.

A configuração vale apenas no processo atual e no workspace ativo. Reiniciar o
servidor preserva a seleção; trocar projeto, desativar/remover/reinstalar Rust ou
fechar o aplicativo descarta a autorização. Cancelar a revisão não altera a análise.
Os diretórios são concessões de leitura, não snapshots imutáveis de conteúdo ou
de executáveis: atualizações concorrentes no mesmo local não possuem garantia de
identidade por hash. Selecionar uma toolchain pressupõe confiança na instalação.

## Processo e dependências

As ferramentas selecionadas são montadas read-only em `/toolchain` (ou já estão
em `/usr`). PATH, CARGO e RUSTC apontam explicitamente para elas. O processo mantém
os namespaces isolados, rede bloqueada, workspace read-only e HOME/target temporários
da ADR-0021. Wrappers rustc herdados são esvaziados. O registry aprovado é montado
read-only dentro de um Cargo home temporário, que permite locks efêmeros sem
escrever no cache original.

Cargo recebe `--offline --locked`; o IDE não baixa dependências nem cria/atualiza
Cargo.lock. A resolução completa depende de um lockfile preparado e de todas as
fontes necessárias já estarem disponíveis no cache ou no workspace (incluindo
vendor/path dependencies). Dependências Git e paths fora do workspace não recebem
novos mounts nesta etapa. Configuração de fontes do próprio projeto continua
visível ao Cargo e pode precisar ser compatível com os caminhos do sandbox.

Build scripts, check-on-save e proc macros continuam desabilitados. O servidor
pode recarregar metadados ao analisar dependências/biblioteca padrão. A recarga é
necessária também na descoberta inicial de fontes: com cargo.autoreload=false,
a versão testada carregava os metadados mas deixava a resolução externa incompleta.
A correção foi verificada com o servidor real. Toda recarga continua offline,
locked e sujeita aos mesmos mounts e limites de processo.

Estado de saúde do rust-analyzer aparece no painel em texto limitado (4 KiB),
incluindo falhas de metadados. A ausência de diagnósticos não certifica cargo check.
Os riscos residuais de configuração concorrente do workspace da ADR-0021 continuam
válidos; a autoridade efetiva é a fronteira de filesystem/rede do sandbox.

## Fontes externas e navegação

Hover pode usar tipos/documentação das fontes carregadas. Definições fora do
workspace só são aceitas nas raízes de biblioteca padrão/registry-src autorizadas
para a sessão. URI canônica, extensão Rust, ranges UTF-16 e limites de conteúdo
continuam obrigatórios. Leituras usam descritores de diretórios e O_NOFOLLOW,
recusando symlinks e arquivos não regulares.

A resposta contém texto limitado e um rótulo, sem comando para leitura de paths
arbitrários. F12 abre a fonte externa em um visualizador Monaco somente leitura,
sem adicioná-la às abas, rascunhos ou recuperação do workspace. Fechar ou revogar a
análise descarta o modelo. Navegação dentro desse visualizador e edição de
bibliotecas não estão implementadas. O limite é de 512 KiB por arquivo e 8 MiB
por resposta, com até 32 destinos.

## Validação e referências

Testes cobrem descoberta sem execução, exclusão de diretórios pessoais indevidos,
revisão vinculada/expirada/reutilizada, revalidação de paths, URIs externas e
symlinks. A jornada nativa usa ferramentas reais copiadas para uma estrutura de
toolchain temporária, registry isolado com crate de teste e rust-src opcional;
valida hover, F12 externo, readonly, Cargo.lock preservado e revogação. Isso não
certifica todas as versões/toolchains ou projetos com build scripts/proc macros.

- [Configuração do rust-analyzer](https://rust-analyzer.github.io/book/configuration)
- [Cargo home](https://doc.rust-lang.org/cargo/guide/cargo-home.html)
- [Configuração Cargo](https://doc.rust-lang.org/cargo/reference/config.html)
- [Overrides do rustup](https://rust-lang.github.io/rustup/overrides.html)
