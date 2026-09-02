# Manifesto de plugins do Lyrnova

Status: contrato inicial da API de plugins v1.

Cada plugin possui um `plugin.json` validado pelo núcleo antes de aparecer no
catálogo. Campos desconhecidos, versões incompatíveis, enums desconhecidos,
IDs duplicados, URLs fora de HTTPS/GitHub, entrypoints inseguros e checksums
inválidos são rejeitados. O schema de referência está em
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
nome de asset simples e SHA-256 em hexadecimal minúsculo. Nesta fase, o núcleo
apenas valida o contrato; download, extração e execução externa continuam
desativados. A origem do documento é fornecida separadamente ao parser: um
pacote externo não pode se declarar `bundled` para evitar checksum ou obter o
selo de plugin oficial.

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
