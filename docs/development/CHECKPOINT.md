# Checkpoint de desenvolvimento

Data: 2026-09-02
Commit-base: `d081f3c`
Estado: lifecycle sandboxed e catálogo autenticado de plugins implementados e validados.

## Direção consolidada

- Lyrnova é um IDE desktop extensível, não um cliente Codex.
- Linguagens, frameworks, runtimes e provedores de IA são plugins opcionais.
- A instalação inicial não inclui nem ativa o Codex.
- O projeto usa Rust + Tauri 2 e uma interface inspirada no fluxo familiar do VS Code/Codex.
- Nada deve ser publicado, enviado ao GitHub ou ao OBS sem autorização explícita.

## Último trabalho concluído localmente

- Contrato v1 de `plugin.json` implementado com schema JSON e validação Rust
  estrita.
- Catálogo hardcoded substituído por manifests embutidos e validados para Rust,
  Web Essentials e Codex.
- Capabilities, permissões, tipos, runtime e origem passaram a usar enums fechados.
- Estado de plugins migrado para v3 com concessões separadas; mudança de permissões
  desabilita o plugin até nova revisão.
- Instalação exige aprovação exata das permissões e operações sensíveis do adapter
  Codex verificam declaração e concessão.
- Manifests externos não podem se declarar embutidos; processos exigem entrypoint
  relativo, protocolo conhecido e `process_spawn`.
- Pacotes externos `.tar.zst` usam descritor SHA-256 separado do manifesto para
  evitar a circularidade de um arquivo declarar o próprio hash.
- Instalador local em duas fases implementado: staging/inspeção e instalação após
  aprovação exata das permissões.
- Extração limitada por tamanho comprimido, fluxo descomprimido, arquivo, soma e
  quantidade; traversal, paths não UTF-8, links, tipos especiais e duplicatas falham.
- Manifesto externo e entrypoint são revalidados no staging; falha ou abandono limpa
  temporários; instalação usa rename atômico, não substitui versão e nasce desabilitada
  e sem bit de execução.
- Instalações externas recebem recibo host-managed e SHA-256 determinístico da árvore,
  cobrindo paths, tipos, modos, tamanhos e conteúdo.
- O catálogo dinâmico redescobre e revalida pacotes a cada reload, escolhe a versão
  SemVer mais recente e impede que externos substituam IDs embutidos.
- Corrupção ou layout inválido falha fechado para todo o catálogo externo, removendo-o
  também do estado de autoridade em memória.
- Estado de plugins migrado para v4 com versões instaladas; upgrades removem grants e
  habilitação até nova revisão; aprovação externa pode ser persistida sem ativação.
- Alterações de instalação, remoção e habilitação só entram em memória depois de a
  persistência ser concluída.
- Configurações agora listam o catálogo real e permitem selecionar um `.tar.zst`
  por diálogo nativo, revisar manifesto, integridade e permissões e confirmar ou
  cancelar a instalação.
- O frontend não envia paths de pacote: recebe uma revisão tipada e um token opaco
  para o único staging pendente. O núcleo exige token válido e aprovação exata.
- Cancelamento e substituição limpam o staging; instalações confirmadas entram no
  catálogo, mas continuam desabilitadas até ativação explícita.
- O catálogo v2 valida versão, expiração, limiar Ed25519 da raiz e assinaturas
  delegadas de publishers sobre JSON canônico. Replay, rollback, downgrade,
  adulteração e chaves revogadas falham fechados antes da persistência atômica.
- O recibo distingue pacotes locais não autenticados de releases assinadas e a
  identidade do publisher é revalidada em todo reload. Updates remotos ficam
  bloqueados até a chave pública raiz oficial ser provisionada no aplicativo.
- Dados do manifesto são renderizados como texto na interface, sem HTML não confiável.
- A remoção externa move atomicamente o diretório completo do ID para uma quarentena,
  evitando que versões antigas sejam promovidas após a exclusão.
- Catálogo, habilitação e concessões são persistidos antes da limpeza; falha restaura
  o pacote, e falha de rollback remove toda autoridade externa da memória.
- Instalação e remoção usam o mesmo lock global, e resíduos de uma interrupção são
  limpos antes da descoberta na próxima inicialização.
- A interface oferece remoção somente para plugins externos e exige confirmação.
- Catálogo v1 estrito e embarcado adicionado como raiz de confiança para downloads;
  ele começa vazio até uma release real ser revisada.
- O frontend solicita download apenas por ID. URL de GitHub Release, tag, descritor,
  hash e destino permanecem sob controle do núcleo Rust.
- Downloads HTTPS verificam allowlist em cada redirect, usam timeout, diretório
  privado e limite de 64 MiB durante o streaming; parciais são limpos no reinício.
- Versões iguais e downgrades são bloqueados antes da rede e novamente sob o lock;
  pacotes baixados passam pela mesma revisão e nascem desabilitados.
- Broker Linux de runtimes externos usa Bubblewrap obrigatório e falha fechado nas
  demais plataformas ou quando o sandbox está ausente.
- Policy engine materializa `workspace_read`, `workspace_write` e `network_access`
  como mounts/rede do sandbox; ambiente, HOME e secrets permanecem isolados.
- Entrypoints continuam não executáveis no pacote e recebem cópia privada somente
  durante a sessão, com limites de recursos e cleanup de lifecycle.
- Ativação inicia o runtime; desativação, remoção, troca de workspace e encerramento
  terminam o processo. Falha no reinício desabilita o plugin externo.
- Licença do código autoral migrada de MIT para `GPL-3.0-only`.
- Texto integral oficial da GNU GPL versão 3 instalado em `LICENSE`.
- Metadados Cargo, npm, AppStream e RPM atualizados, sem alterar as licenças das
  dependências.
- Documentação de escopo e README atualizados para refletir a nova licença.
- Área de configurações implementada.
- Configurações funcionais para fonte do editor, família tipográfica, tamanho das tabs,
  quebra de linha, espaços em branco, minimapa, ligaturas, rolagem suave, fonte do
  terminal e confirmação ao fechar arquivo modificado.
- Configurações são persistidas localmente e aplicadas ao Monaco em tempo real.
- Atalho `Ctrl+,`, entrada na paleta de comandos e botão de engrenagem adicionados.
- Sugestões futuras exibidas para tema/acessibilidade, atalhos, salvamento/formatação,
  perfis do terminal, Git e privacidade dos plugins.
- Tela vazia do editor mostra o ícone do Lyrnova quando todas as abas são fechadas.

## Validações já executadas em 2026-09-02

- `npm run check --prefix ui`
- `npm run build --prefix ui`
- `cargo fmt --all -- --check`
- `cargo check --workspace --offline`
- `cargo clippy --workspace --all-targets --offline -- -D warnings`
- `cargo test --workspace --offline` (94 testes aprovados e 1 teste de integração
  opcional ignorado por exigir Codex App Server local)
- `cargo metadata --offline --no-deps --format-version 1`
- `npm install --package-lock-only --ignore-scripts --offline --prefix ui`
- `appstreamcli validate --no-net`
- `xmllint --noout`
- `rpmspec -P`
- `git diff --check`

O smoke test de runtime iniciou e encerrou um processo real no Bubblewrap. O
contêiner de desenvolvimento permite os namespaces principais com rede concedida,
mas bloqueia a criação do namespace de rede isolado; a negação de rede foi validada
pela construção determinística da política e permanece fail-closed no lançamento.

Todas passaram. Em uma validação visual anterior, a área base de configurações foi
acionada no WebView e também renderizada a partir do bundle local para inspeção em
1600 × 1000. Hierarquia, espaçamento, contraste, controles e convivência com o painel
do terminal foram revisados sem defeitos visuais bloqueantes.

## Ponto exato da licença

O código autoral usa a expressão SPDX `GPL-3.0-only`. O `LICENSE` tem 674 linhas e
corresponde byte a byte ao texto oficial da GNU GPL versão 3 instalado no sistema
(SHA-256 `3972dc9744f6499f0f9b2dbf76696f2ae7ad8af9b23dde66d6af86c9dfb36986`).
Referências a MIT, MPL, Apache, ISC e BSD que permanecem no lockfile pertencem a
dependências e não devem ser substituídas.

## Ao retomar

1. preservar a decisão `GPL-3.0-only` em novos metadados e templates;
2. conectar o transporte estruturado do protocolo externo por capability;
3. manter providers de IA opcionais e ausentes da instalação inicial;
4. continuar sem commit, push, publicação ou envio ao OBS sem autorização explícita;
5. criar um manual para terceiros desenvolverem plugins, cobrindo manifesto,
   permissões, empacotamento, sidecar SHA-256, testes e publicação.

## Estado do repositório

O commit `d081f3c` contém os fluxos de instalação, remoção e download. Este
checkpoint acrescenta o broker sandboxed, o catálogo autenticado e os ADRs 0011
e 0012; nenhum pacote OBS ou release foi realizado.
