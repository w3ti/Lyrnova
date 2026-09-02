# Manifesto de plugins do Lyrnova

Status: contrato inicial da API de plugins v1.

Cada plugin possui um `plugin.json` validado pelo núcleo antes de aparecer no
catálogo. Campos desconhecidos, versões incompatíveis, enums desconhecidos,
IDs duplicados, URLs fora de HTTPS/GitHub e entrypoints inseguros são
rejeitados. O schema de referência está em
[`plugin-manifest.schema.json`](plugin-manifest.schema.json).

## Identidade e compatibilidade

- `schemaVersion` identifica a versão do formato e atualmente deve ser `1`;
- `id` é global, minúsculo e namespaced, por exemplo
  `io.github.w3ti.lyrnova.language.rust`;
- `version` usa SemVer;
- `compatibility.lyrnova` usa uma expressão SemVer e precisa aceitar a versão
  atual do IDE;
- `compatibility.pluginApi` atualmente deve ser `1`.

Incompatibilidade sempre falha fechada. Não há fallback permissivo para uma
versão de schema, API ou protocolo desconhecida.

## Runtime e origem

`runtime.type` pode ser `builtin` ou `process`. Um runtime `process` exige um
entrypoint relativo e normal, protocolo conhecido e a permissão
`process_spawn`. Paths absolutos, `..`, barras invertidas e drive letters são
recusados antes que um processo possa ser considerado.

`source.type` pode ser `bundled` ou `github_release`. Releases externos exigem
nome de asset simples. O SHA-256 do pacote fica em um descritor externo,
proveniente do catálogo ou de metadados confiáveis da release, porque o hash de
um arquivo não pode ser declarado dentro do próprio arquivo sem criar uma
dependência circular. O instalador exige que o asset do descritor corresponda
ao arquivo selecionado e ao manifesto antes de aceitar o pacote.
O formato desse documento separado está em
[`plugin-package-descriptor.schema.json`](plugin-package-descriptor.schema.json).

O catálogo curado v1 é embarcado no aplicativo e validado antes de ser exibido.
Cada entrada liga manifesto, descritor e tag de uma GitHub Release; a URL do asset
é derivada pelo núcleo e nunca fornecida pelo frontend. O formato está em
[`plugin-catalog.schema.json`](plugin-catalog.schema.json). O catálogo distribuído
permanece vazio até existir uma release real revisada, evitando exemplos baixáveis
ou hashes fictícios.

A origem do documento é fornecida separadamente ao parser: um pacote externo
não pode se declarar `bundled` para evitar a verificação de integridade ou obter
o selo de plugin oficial.

## Capabilities e permissões

Capabilities descrevem superfícies funcionais. Permissões representam
autoridade sensível e são enums fechados. A presença de uma capability não
concede a permissão correspondente.

Na instalação, o conjunto aprovado precisa corresponder exatamente às
permissões do manifesto. As concessões são persistidas separadamente. Se uma
atualização mudar as permissões pedidas, o plugin é desativado até nova revisão.
Uma chamada só é autorizada quando o plugin está instalado, habilitado, declara
a permissão e possui a concessão persistida.

## Estado seguro atual

Rust e Web Essentials continuam embutidos e habilitados inicialmente com
`workspace_read`. O Codex permanece ausente da instalação inicial. Seu
manifesto declara processo, rede, leitura do workspace e solicitação de
approval, e cada entrada do adapter verifica as permissões necessárias.

Pacotes externos locais usam `.tar.zst` e passam por staging privado. O núcleo
limita o pacote comprimido, o fluxo descomprimido, cada arquivo, o total
extraído e a quantidade de entradas; recusa traversal, paths não UTF-8, links,
tipos especiais e entradas duplicadas. Depois valida novamente o manifesto e
confere a existência do entrypoint. A instalação só ocorre após aprovação
exata das permissões, por rename atômico, e permanece desabilitada.

Na instalação local, o seletor nativo recebe `exemplo.tar.zst` e procura
`exemplo.tar.zst.json` na mesma pasta. O frontend recebe manifesto, descritor e
métricas para revisão, mas não recebe nem fornece paths. A confirmação usa um
token efêmero mantido pelo núcleo e repete a comparação exata das permissões
antes de consumir o staging. Cancelamento, nova seleção ou encerramento do
aplicativo descartam o staging pendente.

A remoção física está conectada apenas para plugins externos. Ela move por rename
atômico o diretório completo do ID para uma quarentena fora do catálogo, revoga
habilitação e concessões e persiste o novo catálogo antes da limpeza. Falhas de
persistência restauram o diretório; uma interrupção depois do rename é concluída
na próxima inicialização. Todas as versões são removidas juntas para impedir que
uma versão antiga reapareça silenciosamente.

Downloads do catálogo curado estão conectados pelo núcleo Rust. Eles aceitam apenas
um ID, usam HTTPS com redirects limitados a hosts de releases do GitHub, impõem o
limite de 64 MiB durante o streaming e gravam em diretório privado e temporário.
Versões iguais e downgrades são recusados antes e depois da rede. O pacote baixado
segue pelo mesmo staging, revisão e instalação desabilitada do fluxo local. Execução
externa continua não conectada.

Na reinicialização, o catálogo externo é reconstruído apenas de instalações com
recibo válido. O núcleo recalcula o SHA-256 da árvore, incluindo paths, tipos,
modos, tamanhos e conteúdo, e valida novamente o manifesto e o entrypoint. A
versão SemVer mais recente de cada ID aparece como instalada e desabilitada.
Atualizações limpam habilitação e concessões; a aprovação exata pode ser
persistida separadamente sem ativar o plugin. Qualquer pacote inválido faz o
catálogo externo inteiro falhar fechado, preservando somente os embutidos.
