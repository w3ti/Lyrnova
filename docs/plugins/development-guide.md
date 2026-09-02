# Manual de desenvolvimento de plugins

Status: guia para a API de plugins v1 do Lyrnova 0.1.x.

Este manual descreve como um terceiro pode criar, testar, empacotar e preparar
para publicação um plugin externo. O contrato normativo continua sendo formado
pelos schemas e pelo código do host; se houver divergência, o host falha fechado.

## Estado atual da plataforma

Plugins externos podem ser instalados localmente como `.tar.zst`, revisados pelo
usuário e executados no Linux dentro do Bubblewrap. O lifecycle, o sandbox e o
transporte JSONL v1 estão implementados.

A API v1 já fixa os nomes das capabilities e a fronteira de mensagens, mas os
payloads funcionais de LSP, tasks, testes e outras integrações ainda estão sendo
estabilizados. Um plugin consegue validar instalação, ativação, handshake,
request/response e encerramento; ele só aparecerá em uma superfície do IDE quando
existir um consumidor tipado para sua capability. Providers de IA externos também
precisam de um adapter tipado e, por enquanto, não substituem o adapter builtin do
Codex.

O catálogo curado remoto permanece bloqueado até a cerimônia das chaves raiz
oficiais. Portanto, a distribuição disponível hoje é a instalação local, marcada
como não autenticada. Não apresente um pacote como oficial ou curado antes de ele
entrar na cadeia de confiança do projeto.

## Pré-requisitos

Para desenvolver e executar o exemplo deste guia no Linux:

- Lyrnova compilado a partir deste repositório;
- Bubblewrap disponível em `/usr/bin/bwrap`;
- `tar`, `zstd` e `sha256sum` para gerar o pacote;
- o runtime exigido pelo seu entrypoint, ou um binário autocontido;
- um repositório público no GitHub para a identidade declarada no manifesto.

O host atual não inicia runtimes externos fora do Linux. Nessas plataformas, a
instalação pode ser inspecionada, mas a ativação falha fechada até existir um
sandbox equivalente.

## Estrutura mínima

Crie um diretório de trabalho sem links simbólicos ou arquivos especiais:

```text
lyrnova-example-tasks/
├── plugin.json
└── bin/
    └── example-tasks
```

`plugin.json` precisa estar na raiz do arquivo TAR, não dentro de um diretório
adicional. O entrypoint pode ficar em qualquer path relativo normal declarado no
manifesto. Somente arquivos regulares e diretórios são aceitos no pacote.

## Exemplo completo de manifesto

```json
{
  "schemaVersion": 1,
  "id": "io.github.example.lyrnova.tool.tasks",
  "name": "Example Tasks",
  "description": "Expõe tasks de exemplo pelo protocolo do Lyrnova.",
  "version": "0.1.0",
  "publisher": "example",
  "license": "GPL-3.0-only",
  "kind": "tool",
  "compatibility": {
    "lyrnova": ">=0.1.0, <0.2.0",
    "pluginApi": 1
  },
  "runtime": {
    "type": "process",
    "entrypoint": "bin/example-tasks",
    "protocolVersion": 1
  },
  "source": {
    "type": "github_release",
    "repository": "https://github.com/example/lyrnova-example-tasks",
    "asset": "lyrnova-example-tasks-0.1.0-linux-x86_64.tar.zst"
  },
  "capabilities": ["tasks"],
  "permissions": ["workspace_read", "process_spawn"]
}
```

Substitua `example`, o repositório, a licença e os demais metadados por valores
reais. O nome em `source.asset` deve ser idêntico ao nome do pacote final.

### Campos obrigatórios

| Campo | Regra principal |
| --- | --- |
| `schemaVersion` | Deve ser `1`. |
| `id` | Identificador global minúsculo com ao menos três segmentos, como `io.github.autor.plugin`. |
| `name` | Nome visível, entre 1 e 80 bytes, sem controles. |
| `description` | Texto visível, entre 1 e 512 bytes, sem controles. |
| `version` | Versão SemVer; cada atualização precisa aumentá-la. |
| `publisher` | Segmento minúsculo de até 63 bytes; deve corresponder à identidade publicada. |
| `license` | Expressão SPDX válida para o código distribuído. |
| `kind` | `language`, `runtime`, `framework`, `tool` ou `ai_provider`. |
| `compatibility.lyrnova` | Expressão SemVer que precisa aceitar a versão do host. |
| `compatibility.pluginApi` | Deve ser `1`. |
| `runtime` | Para terceiros, use `process`, entrypoint relativo e protocolo `1`. |
| `source` | Para terceiros, use `github_release`, repositório GitHub HTTPS e asset simples. |
| `capabilities` | Lista sem duplicatas das superfícies funcionais utilizadas. |
| `permissions` | Lista sem duplicatas da autoridade solicitada. |

`runtime.type: builtin` e `source.type: bundled` são reservados a componentes
compilados e distribuídos com o Lyrnova. Um pacote externo que declarar origem
`bundled` será recusado.

Plugins com capability `tasks` implementam `tasks.list` conforme o contrato em
[`protocol.md`](protocol.md). Cada Task descreve um comando estruturado, cwd,
environment permitido, nível de acesso, rede e timeout. O frontend nunca envia
esses campos de volta: ele seleciona `pluginId + taskId`, e o núcleo consulta o
runtime novamente, deriva autoridade dos grants persistidos e cria a revisão.

Declare sempre `workspace_read` e `process_spawn`. Acrescente `workspace_write`
somente se alguma Task realmente modificar o projeto e `network_access` somente se
ela precisar de rede. Tasks não podem solicitar execução elevada no host.

O schema completo está em
[`plugin-manifest.schema.json`](plugin-manifest.schema.json). Campos desconhecidos,
paths absolutos, `..`, barras invertidas, drive letters, URLs com query ou fragmento
e repositórios fora de `https://github.com/<owner>/<repo>` são recusados.

## Capabilities

Capabilities descrevem dados e operações, não concedem autoridade. A API v1 aceita:

| Área | Capabilities |
| --- | --- |
| Linguagem e edição | `syntax_highlighting`, `autocomplete`, `diagnostics`, `snippets`, `templates` |
| Ferramentas de desenvolvimento | `lsp`, `dap`, `tasks`, `tests` |
| Providers de IA | `account_auth`, `ai_chat`, `ai_tools`, `approvals` |

Toda operação deve começar com o nome exato da capability e um ponto. Exemplos
válidos são `tasks.list`, `tests.discover` e `diagnostics.read`. `tasks.list` não
pode ser enviado sob a capability `diagnostics`.

Declare somente capabilities implementadas. No handshake, o plugin precisa repetir
exatamente o conjunto do manifesto; itens ausentes, extras ou duplicados encerram o
processo.

## Permissões

Solicite o menor conjunto possível. Na instalação, o usuário precisa aceitar
exatamente todas as permissões declaradas; uma mudança futura revoga os grants e
desabilita o plugin até nova revisão.

| Permissão | Efeito atual |
| --- | --- |
| `process_spawn` | Obrigatória para qualquer runtime `process`; autoriza o entrypoint declarado e seus subprocessos dentro do mesmo sandbox. |
| `workspace_read` | Monta o projeto ativo em `/workspace` somente para leitura. |
| `workspace_write` | Monta o projeto ativo em `/workspace` para leitura e escrita. |
| `network_access` | Compartilha a rede do host e disponibiliza configuração de DNS/certificados no sandbox. |
| `secret_storage` | Reserva acesso futuro ao serviço de secrets mediado pelo host; não expõe HOME ou cofre diretamente. |
| `request_approval` | Reserva o uso de approvals mediados pelo host; não autoriza o efeito solicitado. |

Sem permissão de workspace, `/workspace` existe como diretório vazio. Sem
`network_access`, o namespace de rede fica isolado. `secret_storage` e
`request_approval` não criam automaticamente operações no protocolo; dependem de
um serviço tipado do host.

## Implementar o entrypoint

O processo lê um objeto JSON por linha em stdin e escreve um objeto JSON por linha
em stdout. Stdout é reservado ao protocolo; envie logs para stderr, lembrando que o
host o descarta na execução normal. Sempre faça flush depois de cada frame.

Este entrypoint Python demonstra o handshake e `tasks.list`. Ele serve para testes
de transporte e pressupõe `python3` instalado no sistema; para distribuição, prefira
um binário autocontido ou documente claramente suas dependências.

```python
#!/usr/bin/env python3
import json
import sys

CAPABILITIES = ["tasks"]


def emit(frame):
    sys.stdout.write(json.dumps(frame, separators=(",", ":")) + "\n")
    sys.stdout.flush()


line = sys.stdin.readline()
if not line:
    raise SystemExit(1)

initialize = json.loads(line)
if (
    initialize.get("type") != "initialize"
    or initialize.get("protocol_version") != 1
    or sorted(initialize.get("capabilities", [])) != sorted(CAPABILITIES)
):
    raise SystemExit(2)

emit({
    "type": "ready",
    "protocol_version": 1,
    "capabilities": CAPABILITIES,
})

for line in sys.stdin:
    frame = json.loads(line)
    if frame.get("type") == "shutdown":
        raise SystemExit(0)
    if frame.get("type") != "request":
        raise SystemExit(3)

    request_id = frame.get("request_id")
    capability = frame.get("capability")
    if capability == "tasks" and frame.get("operation") == "tasks.list":
        emit({
            "type": "response",
            "request_id": request_id,
            "capability": capability,
            "result": {
                "items": [{"id": "example.build", "label": "Build example"}]
            },
        })
    else:
        emit({
            "type": "error",
            "request_id": request_id,
            "capability": capability,
            "code": "unsupported_operation",
            "message": "Operation is not implemented by this plugin.",
        })
```

O protocolo completo, inclusive formatos de response, error e event, está em
[`protocol.md`](protocol.md).

### Ambiente do sandbox

O runtime inicia com:

- pacote montado somente para leitura em `/plugin`;
- diretório de trabalho `/plugin`;
- workspace em `/workspace`, conforme os grants;
- `HOME=/tmp/home` e `TMPDIR=/tmp`, ambos efêmeros;
- `PATH=/usr/bin:/bin` e `LANG=C.UTF-8`;
- `LYRNOVA_PLUGIN_ID`, `LYRNOVA_PLUGIN_VERSION`,
  `LYRNOVA_PLUGIN_PROTOCOL_VERSION` e `LYRNOVA_WORKSPACE`;
- ambiente original, HOME real, secrets e demais paths do usuário removidos.

O host cria uma cópia privada executável do entrypoint durante a sessão. Não
dependa do bit executável preservado no pacote. O processo recebe limites de 256
arquivos abertos, arquivos de até 64 MiB, 2 GiB de espaço de endereçamento, core
dump desabilitado e `no_new_privs`.

O handshake expira em 3 segundos, cada request em 30 segundos e o shutdown normal
tem uma janela de 250 ms. Guarde estado importante antes de receber `shutdown`.

## Limites do protocolo

- cada frame JSONL tem no máximo 256 KiB e termina obrigatoriamente em `\n`;
- IDs, operações, eventos e códigos têm até 128 bytes e usam ASCII alfanumérico,
  `.`, `_`, `-` ou `:`;
- mensagens de erro têm entre 1 e 1.024 bytes;
- strings de payload têm até 128 KiB;
- objetos e arrays têm até 4.096 itens e profundidade máxima 16;
- números inteiros são aceitos; floats e NUL são proibidos;
- no máximo 128 eventos podem ficar pendentes no host.

EOF, timeout, resposta com ID/capability divergente, frame fora de ordem ou evento
não declarado são violações fatais. Uma falha funcional conhecida deve usar um
frame `error`, não encerrar ou corromper a sessão.

## Gerar o pacote

Antes de empacotar, confirme que `source.asset` contém o nome definitivo do asset.
O exemplo abaixo usa GNU tar e gera metadados reprodutíveis:

```bash
mkdir -p dist
tar \
  --sort=name \
  --mtime='UTC 1970-01-01' \
  --owner=0 \
  --group=0 \
  --numeric-owner \
  -C lyrnova-example-tasks \
  -cf - \
  plugin.json bin \
  | zstd -19 --threads=0 \
      -o dist/lyrnova-example-tasks-0.1.0-linux-x86_64.tar.zst
```

Não inclua symlinks, hardlinks, sockets, devices ou FIFOs. Os limites atuais são:

- pacote comprimido: 64 MiB;
- stream TAR descomprimido: 300 MiB;
- soma dos arquivos extraídos: 256 MiB;
- arquivo ou entrypoint individual: 64 MiB;
- quantidade de entradas: 4.096;
- path interno: 240 bytes.

Inspecione a raiz antes de gerar o descritor:

```bash
tar --zstd -tf dist/lyrnova-example-tasks-0.1.0-linux-x86_64.tar.zst
sha256sum dist/lyrnova-example-tasks-0.1.0-linux-x86_64.tar.zst
```

A listagem deve mostrar `plugin.json` e `bin/example-tasks` diretamente, sem o
prefixo `lyrnova-example-tasks/`.

## Criar o sidecar SHA-256

Ao lado do pacote, crie um arquivo com o mesmo nome acrescido de `.json`:

```text
dist/
├── lyrnova-example-tasks-0.1.0-linux-x86_64.tar.zst
└── lyrnova-example-tasks-0.1.0-linux-x86_64.tar.zst.json
```

Seu conteúdo é estrito e não aceita campos adicionais:

```json
{
  "asset": "lyrnova-example-tasks-0.1.0-linux-x86_64.tar.zst",
  "sha256": "SUBSTITUA_PELO_SHA256_MINUSCULO_DE_64_CARACTERES"
}
```

O campo `asset` precisa coincidir com o nome do arquivo, com `source.asset` do
manifesto e, futuramente, com o descritor assinado do catálogo. Calcule o hash
somente depois que o pacote estiver finalizado; qualquer alteração exige um novo
hash. O schema está em
[`plugin-package-descriptor.schema.json`](plugin-package-descriptor.schema.json).

## Testar

### 1. Manifesto e layout

Valide primeiro a sintaxe JSON e inspecione o pacote:

```bash
python3 -m json.tool lyrnova-example-tasks/plugin.json >/dev/null
tar --zstd -tf dist/lyrnova-example-tasks-0.1.0-linux-x86_64.tar.zst
sha256sum --check <(printf '%s  %s\n' \
  'SHA256_DO_SIDECAR' \
  'dist/lyrnova-example-tasks-0.1.0-linux-x86_64.tar.zst')
```

Se usar um validador JSON Schema compatível com Draft 2020-12, valide o manifesto
contra [`plugin-manifest.schema.json`](plugin-manifest.schema.json) e o sidecar
contra
[`plugin-package-descriptor.schema.json`](plugin-package-descriptor.schema.json).
A instalação pelo Lyrnova continua sendo a validação normativa, pois também aplica
compatibilidade, origem, layout e limites que atravessam mais de um documento.

### 2. Handshake isolado

Alimente o entrypoint diretamente, sem usar dados reais:

```bash
printf '%s\n' \
  '{"type":"initialize","protocol_version":1,"plugin_id":"io.github.example.lyrnova.tool.tasks","plugin_version":"0.1.0","capabilities":["tasks"],"permissions":["workspace_read","process_spawn"]}' \
  '{"type":"shutdown"}' \
  | python3 lyrnova-example-tasks/bin/example-tasks
```

A única saída esperada é um frame `ready` válido. Acrescente testes automatizados
para request/response, error, eventos, EOF e frames inválidos na linguagem do seu
plugin.

### 3. Casos negativos mínimos

Teste que seu runtime:

- recusa protocolo diferente de `1`;
- não anuncia capability extra ou duplicada;
- nunca grava logs em stdout;
- repete `request_id` e `capability` nas respostas;
- limita payloads antes de alocar ou processar conteúdo;
- encerra limpo ao receber `shutdown`;
- funciona com HOME vazio, ambiente mínimo e package read-only;
- continua seguro sem rede e sem workspace;
- não lê nem escreve fora de `/workspace` quando esse mount é concedido.

### 4. Instalação local

No Lyrnova, abra **Configurações → Plugins → Selecionar pacote** e escolha o
`.tar.zst`. O sidecar precisa estar na mesma pasta. Revise manifesto, integridade e
todas as permissões; a instalação nasce desabilitada. Depois, ative o plugin em uma
ação separada.

No Linux, a ativação deve concluir o handshake no Bubblewrap. Trocar de workspace,
desabilitar, remover ou encerrar o aplicativo encerra o runtime. Reabra o Lyrnova
para verificar também a revalidação do recibo e da árvore instalada.

Alterar manualmente arquivos já instalados não é um fluxo de desenvolvimento: o
hash da árvore deixa de coincidir e o catálogo externo falha fechado. Gere uma nova
versão SemVer e reinstale um novo pacote.

## Atualizar um plugin

Para cada release:

1. aumente `version` no manifesto;
2. ajuste `compatibility` somente se necessário;
3. atualize `source.asset` para o novo nome;
4. gere o pacote novamente;
5. calcule um novo SHA-256 e sidecar;
6. repita todos os testes;
7. revise qualquer alteração de capabilities ou permissões.

O Lyrnova recusa reinstalar a mesma versão. Downloads curados recusam versões iguais
e downgrades; numa instalação local, uma versão inferior não substitui a versão
SemVer mais recente já descoberta. Uma mudança nas permissões remove a habilitação
e os grants até o usuário realizar outra revisão. Não reutilize uma tag ou um asset
publicado com conteúdo diferente.

## Preparar a publicação

### Distribuição local

Distribua sempre o par `.tar.zst` + `.tar.zst.json`. Essa modalidade confirma
integridade, mas não autentica a identidade do publisher; a interface a identifica
como pacote local não autenticado.

### GitHub Release

Para preparar uma futura entrada no catálogo curado:

1. mantenha `source.repository` em um repositório público GitHub com exatamente
   dois segmentos de path;
2. crie uma tag simples, como `v0.1.0`, sem `/`, espaços, query ou fragmento;
3. anexe o `.tar.zst` com o nome exato de `source.asset`;
4. publique também o sidecar para auditoria e instalação local;
5. preserve os fontes, lockfiles, licença e instruções de build correspondentes;
6. forneça SBOM e proveniência reproduzível quando o plugin tiver dependências.

O download curado será derivado exclusivamente de repositório, tag e asset
autenticados. URLs arbitrárias não são aceitas.

### Catálogo autenticado

A publicação curada não é autosserviço nesta versão. Ela exige:

- revisão do plugin e de suas permissões;
- delegação de uma chave Ed25519 ao publisher;
- `keyId` igual ao SHA-256 minúsculo dos 32 bytes da chave pública;
- assinatura do payload canônico que liga manifesto, descritor e release tag;
- inclusão em um catálogo v2 com versão crescente e validade limitada;
- assinaturas offline suficientes para atingir o limiar da raiz.

Os domínios de assinatura são `Lyrnova plugin release v1\0` para a release e
`Lyrnova catalog v2\0` para o catálogo. Não implemente esse processo por conta
própria nem envie chaves privadas: o projeto ainda publicará tooling e procedimento
operacional próprios. Chaves privadas devem permanecer sob controle do publisher;
os mantenedores recebem apenas a chave pública e a assinatura destacada.

Quando a curadoria for aberta, envie uma proposta seguindo
[`CONTRIBUTING.md`](../../CONTRIBUTING.md), contendo:

- ID, versão, kind e capabilities;
- justificativa de cada permissão;
- URL do repositório, tag e asset;
- manifesto e sidecar exatos da release;
- plataformas e arquiteturas suportadas;
- comandos e resultados de testes;
- chave pública/`keyId` e assinatura produzida pelo tooling oficial.

O formato futuro da entrada está em
[`plugin-catalog.schema.json`](plugin-catalog.schema.json), e a cadeia de confiança
em [`catalog-trust.schema.json`](catalog-trust.schema.json). Uma pull request não
deve alterar a raiz de confiança nem inventar chaves provisórias.

## Checklist antes de distribuir

- [ ] `plugin.json` está na raiz e passa no schema v1.
- [ ] ID, publisher, versão, repositório, tag e asset são coerentes.
- [ ] O pacote contém apenas diretórios e arquivos regulares necessários.
- [ ] O entrypoint não depende do HOME ou do ambiente do desenvolvedor.
- [ ] Capabilities e permissões são mínimas, sem duplicatas.
- [ ] Handshake, request/response, error e shutdown têm testes.
- [ ] Nenhum log, token, prompt, arquivo ou secret é escrito em stdout.
- [ ] O sidecar contém o SHA-256 final e usa o nome exato do asset.
- [ ] O pacote foi instalado, ativado, reiniciado e removido localmente.
- [ ] Casos sem rede e sem workspace foram testados.
- [ ] Licenças de código e dependências acompanham a distribuição.
- [ ] Nenhuma credencial ou chave privada foi incluída no pacote.

## Solução de problemas

| Sintoma | Verificação |
| --- | --- |
| `invalid_manifest` | Campos extras, origem, URL, SemVer, API, protocolo, ID e `process_spawn`. |
| `checksum_mismatch` | Recalcule o hash do `.tar.zst` final e atualize o sidecar. |
| `invalid_archive` ou path inseguro | Remova prefixo de diretório, links, tipos especiais, `..`, `:` e barras invertidas. |
| `missing_entrypoint` | Confirme o path relativo e que ele é um arquivo regular dentro do TAR. |
| `permission_approval_required` | A revisão precisa aceitar exatamente o conjunto declarado. |
| `sandbox_unavailable` | Instale Bubblewrap em `/usr/bin/bwrap` e confirme suporte a namespaces. |
| `runtime_workspace_unavailable` | Abra um projeto antes de ativar um plugin que pede acesso ao workspace. |
| `runtime_start_failed` | Verifique interpreter/binário, handshake em 3 s, stdout JSONL e capabilities exatas. |
| Plugin some após reiniciar | Não altere a instalação; confirme recibo, versão e integridade do conteúdo. |

## Referências normativas

- [`manifest.md`](manifest.md)
- [`plugin-manifest.schema.json`](plugin-manifest.schema.json)
- [`protocol.md`](protocol.md)
- [`plugin-package-descriptor.schema.json`](plugin-package-descriptor.schema.json)
- [`plugin-catalog.schema.json`](plugin-catalog.schema.json)
- [`catalog-trust.schema.json`](catalog-trust.schema.json)
- [ADR-0005: manifestos e grants](../architecture/ADR-0005-plugin-manifest.md)
- [ADR-0006: instalação transacional](../architecture/ADR-0006-transactional-plugin-install.md)
- [ADR-0011: runtimes sandboxed](../architecture/ADR-0011-sandboxed-plugin-runtimes.md)
- [ADR-0012: catálogo autenticado](../architecture/ADR-0012-authenticated-plugin-catalog.md)
- [ADR-0013: protocolo por capability](../architecture/ADR-0013-capability-scoped-plugin-protocol.md)
- [Threat model](../security/threat-model.md)
