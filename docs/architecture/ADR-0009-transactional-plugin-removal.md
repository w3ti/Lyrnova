# ADR-0009: Remoção transacional de pacotes externos

- Status: aceito
- Data: 2026-09-02

## Contexto

Remover somente a preferência de um plugin externo deixa código e versões antigas
no disco. Apagar diretamente o diretório, por outro lado, impede rollback se a
persistência do catálogo ou das permissões falhar. Remover apenas a versão ativa
também poderia promover silenciosamente uma versão anterior ainda instalada.

## Decisão

A remoção externa opera sobre o diretório inteiro
`plugins/packages/<plugin-id>`, cobrindo todas as versões. O núcleo valida que o
path da versão ativa corresponde exatamente ao layout derivado do ID e da versão
do manifesto. Instalação e remoção compartilham o mesmo lock global de mutação.

O diretório do plugin é movido por rename atômico para `plugins/.removals`, fora
da árvore descoberta pelo catálogo. O núcleo então reconstrói o catálogo, remove
habilitação e concessões e persiste as preferências. Se reconstrução ou
persistência falhar, o rename é revertido e o estado anterior permanece publicado.
Se o rollback físico falhar, todos os plugins externos falham fechados em memória
e a preferência segura é persistida quando possível.

Após o commit do estado, a quarentena é apagada. Uma interrupção entre o rename e
a limpeza é tratada como remoção confirmada: na próxima inicialização, resíduos em
`.removals` são eliminados antes da descoberta. Plugins embutidos mantêm o fluxo
anterior de desinstalação lógica e nunca passam pela remoção física.

## Consequências

- uma versão antiga não reaparece depois de remover a versão mais recente;
- catálogo, habilitação e grants só mudam depois que o pacote sai da árvore ativa;
- falhas normais de persistência restauram os arquivos por rollback;
- uma queda após o rename conclui a remoção no reinício, em vez de reativar código;
- bytes podem permanecer brevemente na quarentena se a limpeza for interrompida,
  mas nunca voltam ao catálogo e são coletados na próxima inicialização;
- a interface oferece remoção somente para pacotes externos e exige confirmação.
