# ADR-0017: Aprovações vinculadas à ação e revogáveis na sessão

- Status: aceita
- Data: 2026-09-02

## Contexto

Um botão de confirmação não é uma fronteira de autoridade se puder ser reaproveitado
para outro comando, path, domínio ou ambiente. Também não é suficiente delegar ao
provider o significado de “permitir na sessão”: uma implementação comprometida
poderia transformar uma escolha estreita em uma política mais ampla ou impossível de
revogar pelo IDE.

Tasks de plugins já tinham revisão em duas fases e token opaco, enquanto approvals do
agente correlacionavam thread, turn e item. Faltavam um vínculo verificável com todos
os detalhes apresentados, expiração explícita, regras locais revogáveis e histórico
consultável sem conteúdo sensível.

## Decisão

Toda aprovação recebe um SHA-256 calculado pelo núcleo sobre uma representação
canônica e tipada da ação. A interface devolve o token e esse mesmo hash; divergência
falha antes do consumo. Para comandos do agente, o vínculo inclui categoria, motivo,
comando, cwd, arquivos/diffs, destino de rede, protocolo e identidade do ambiente.
Para Tasks, inclui origem, comando, cwd, acesso, rede, environment, timeout, risco e
força do sandbox. O token continua de uso único e mudanças de grants invalidam Tasks.

Approvals pendentes expiram em cinco minutos. Timeout, encerramento de sessão, troca
de workspace e logout negam pendências; ausência de resposta nunca significa aceite.
Decisões concorrentes são serializadas sob o estado do broker e somente a primeira
pode consumir a solicitação.

“Permitir na sessão” cria uma regra em memória, limitada à conversa e ao hash exato.
O provider recebe apenas `accept` para a ocorrência corrente. Repetições exatas são
aceitas pelo núcleo; qualquer alteração volta a pedir confirmação. As regras são
listadas nas configurações, podem ser revogadas imediatamente, têm quantidade
limitada e desaparecem com o lifecycle da sessão.

A interface mostra ação, motivo, cwd/raiz, arquivos ou domínio, risco, validade,
escopo e hash. Sugestões do provider para ampliar políticas são explicitamente
ignoradas. Valores de environment não atravessam a projeção; somente seus nomes ou a
identidade opaca do ambiente podem ser indicados como dados redigidos. Ações críticas
usam tratamento visual destrutivo, sem botão primário tranquilizador.

O histórico em memória guarda somente UUID do evento, hash, categoria, decisão,
origem da decisão e horário. Não guarda comando, diff, conversa, environment ou
segredos. Para Tasks, os eventos já produzidos pelo broker acrescentam origem, fase,
política e resultado usando somente o hash do comando.

## Consequências

- o frontend continua sem autoridade para executar efeitos;
- replay, hash trocado, resposta duplicada e aprovação expirada falham fechados;
- providers não controlam nem persistem as regras de sessão do Lyrnova;
- o usuário pode inspecionar e revogar autoridade temporária em Configurações;
- a auditoria permite correlação sem se tornar uma nova fonte de vazamento.

Regras persistentes entre reinicializações não fazem parte desta decisão. Se forem
adicionadas, exigirão storage autenticado, revisão de escopo e expiração próprias.
