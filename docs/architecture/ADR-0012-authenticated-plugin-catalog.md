# ADR-0012: Autenticar publishers e atualizações do catálogo

- Status: aceito
- Data: 2026-09-02

## Contexto

SHA-256 confirma que um pacote corresponde a um descritor, mas não identifica
quem publicou esse descritor. Um catálogo remoto sem versão e validade também
permite replay, rollback e congelamento. Incorporar uma chave privada ao
aplicativo ou confiar apenas em HTTPS não estabelece uma cadeia de confiança
adequada para código executável.

## Decisão

O catálogo v2 usa um envelope com metadados `signed`, versão monotônica,
expiração, chaves delegadas de publishers e assinaturas raiz com limiar. A raiz
de confiança contém somente chaves públicas embarcadas no aplicativo; sua
rotação exige uma nova versão do Lyrnova. Chaves de publisher podem ser
adicionadas, rotacionadas ou revogadas pelo catálogo autenticado.

Raiz e publishers usam Ed25519. O ID de chave é o SHA-256 minúsculo dos 32 bytes
da chave pública. Assinaturas são verificadas estritamente sobre JSON canônico,
precedido por um domínio específico para catálogo ou release. O núcleo rejeita
chaves fracas, campos desconhecidos, floats, assinaturas duplicadas no limiar,
catálogo expirado, versão repetida ou inferior e downgrade SemVer de uma entrada
que permanece publicada.

Uma entrada assina manifesto, descritor SHA-256 e tag de release. Após o download,
manifesto e descritor extraídos precisam ser exatamente iguais aos metadados
assinados. A autenticação é copiada para o recibo host-managed e revalidada em
cada reload contra a chave ativa atual. Revogar a chave faz o pacote assinado
falhar fechado. Instalações locais permanecem possíveis, mas são explicitamente
marcadas como não autenticadas.

Atualizações são buscadas apenas de uma URL fixa de GitHub Releases, validadas
antes de persistir e gravadas por substituição atômica em diretório privado. O
catálogo embarcado continua sendo o fallback compilado. Enquanto a cerimônia da
chave raiz oficial não ocorrer, a lista de chaves públicas fica vazia e a
atualização remota falha fechada com erro explícito; nenhuma chave privada ou
identidade provisória é criada silenciosamente.

## Consequências

- compromisso da hospedagem ou do TLS não basta para publicar metadados aceitos;
- replay, rollback, congelamento e downgrade são detectados;
- publishers controlam a assinatura de suas releases sem controlar a raiz;
- revogação protege reloads futuros e desativa a autoridade do pacote afetado;
- a publicação oficial precisa de procedimento separado para geração, custódia
  e assinatura offline das chaves raiz.

## Referências

- [The Update Framework Specification](https://theupdateframework.github.io/specification/)
- [RFC 8032: Edwards-Curve Digital Signature Algorithm](https://www.rfc-editor.org/rfc/rfc8032)
- [`ed25519-dalek::VerifyingKey::verify_strict`](https://docs.rs/ed25519-dalek/2.2.0/ed25519_dalek/struct.VerifyingKey.html#method.verify_strict)
