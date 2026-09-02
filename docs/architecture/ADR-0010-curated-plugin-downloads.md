# ADR-0010: Downloads por catálogo curado embarcado

- Status: aceito
- Data: 2026-09-02

## Contexto

Aceitar do frontend uma URL e um SHA-256 permitiria trocar simultaneamente o
conteúdo e a expectativa de integridade. Buscar um catálogo remoto sem assinatura
apenas deslocaria essa confiança para uma resposta de rede mutável. O primeiro
download conectado precisa ter uma raiz de confiança simples e auditável.

## Decisão

O catálogo v1 é JSON estrito embarcado no binário e, portanto, versionado e revisado
como parte do aplicativo. Cada entrada contém um manifesto externo completo, o
descritor com SHA-256 e a tag da GitHub Release. Schema, quantidade, IDs únicos,
compatibilidade, origem, asset, hash e tag são validados antes da exposição à UI.
O catálogo distribuído inicialmente fica vazio até uma release real ser curada.

O frontend recebe uma projeção sem URL de download e solicita o pacote somente pelo
ID. O núcleo deriva
`https://github.com/<owner>/<repo>/releases/download/<tag>/<asset>` dos campos
validados. HTTPS, ausência de credenciais/porta/fragmento, até cinco redirects e uma
allowlist exata de hosts de assets do GitHub são verificados em cada salto. A lista
segue os domínios documentados pelo GitHub para downloads de releases.

O corpo é transmitido para arquivo privado com limite de 64 MiB aplicado durante o
streaming, além da verificação antecipada de `Content-Length`. Downloads parciais
são apagados em falhas normais e na próxima inicialização após uma interrupção. O
arquivo baixado ainda passa pelo staging, SHA-256, validação de manifesto e revisão
exata de permissões definidos pelas ADRs 0006 e 0008.

Downgrades e versões iguais são recusados antes do download e novamente sob o lock
global antes de publicar a revisão. A confirmação continua instalando o plugin
desabilitado. O frontend nunca fornece URL, tag, descritor ou path de destino.

## Consequências

- alterar o catálogo exige uma nova versão revisada do Lyrnova;
- TLS protege o transporte e o SHA-256 liga o asset ao catálogo embarcado;
- redirecionamentos para hosts não autorizados falham antes de baixar o corpo;
- o catálogo não autentica criptograficamente publishers de forma independente;
- assinatura de publishers e atualização remota do catálogo permanecem etapas
  posteriores;
- o schema público está em
  [`plugin-catalog.schema.json`](../plugins/plugin-catalog.schema.json).
