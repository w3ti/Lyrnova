# ADR-0013: Transportar mensagens externas por capability

- Status: aceito
- Data: 2026-09-02

## Contexto

O sandbox impede acesso direto além das permissões concedidas, mas um runtime
externo ainda precisa trocar dados funcionais com o núcleo. Expor stdin como
terminal, aceitar comandos livres ou encaminhar JSON arbitrário diretamente ao
frontend criaria uma nova fronteira de autoridade fora do manifesto.

## Decisão

Runtimes `process` usam exclusivamente o protocolo JSONL v1 em stdin/stdout. O
primeiro frame do host é `initialize`; antes de permanecer ativo, o plugin deve
responder `ready` com a mesma versão de protocolo e exatamente o conjunto de
capabilities declarado no manifesto. Ausência, timeout ou divergência encerram o
processo e mantêm o plugin desabilitado.

Chamadas usam IDs imprevisíveis gerados pelo host, uma capability tipada, uma
operação e payload JSON. A operação obrigatoriamente pertence ao namespace da
capability, como `tasks.list` ou `diagnostics.read`. O broker só envia a chamada
se a capability estiver no manifesto revalidado. Respostas e erros precisam
repetir ID e capability; mensagens fora de ordem, `ready` repetido ou capability
não declarada são violações fatais.

Frames têm no máximo 256 KiB e exigem newline. Campos desconhecidos, floats,
NUL, nomes inválidos, estruturas com mais de 16 níveis, strings acima de 128 KiB
e coleções acima de 4.096 itens são recusados. Um reader dedicado usa canal
limitado para aplicar backpressure. Eventos válidos são enfileirados até 128;
excesso encerra a sessão. Chamadas expiram em 30 segundos. `shutdown` recebe uma
janela curta antes de encerramento forçado.

Payloads continuam sendo dados não confiáveis. Este protocolo não oferece ao
plugin chamadas de shell, leitura arbitrária do host, acesso ao frontend ou
efeitos mediados. Cada consumidor futuro deve decodificar o payload em seu tipo
específico e aplicar seus próprios limites antes de alterar estado.

## Consequências

- stdout fica reservado ao protocolo; logs não atravessam essa fronteira;
- protocolo, manifesto e grants são revalidados antes da comunicação;
- uma resposta não pode migrar entre requests ou capabilities;
- timeout, EOF e qualquer violação removem o runtime da tabela ativa;
- o núcleo possui APIs Rust para request/response e drenagem de eventos, sem um
  command Tauri genérico acessível ao frontend.
