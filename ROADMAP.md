# Roadmap for PDU versioning

Currently, PDUs are one monolithic `PduEvent` struct which applies a one-size-fits-all approach to PDUs, however as we
support a wide range of room versions, this only just about currently works, and will cause more and more problems as
time progresses. This branch will split the `PduEvent` struct into multiple versioned structs that represent the proper
format for appropriate room versions, and will drop the `Event` trait in favour of Ruma's `ruma::state_res::Event`
trait.

This will require a lot of work, and since I forget everything the moment I look out the window, here be a list (waow):

- [x] Split `PduEvent` into multiple versioned structs, implementing `ruma::state_res::Event` for each of them.
- [ ] Refactor out all references to `PduEvent`.
- [ ] Refactor out all references to `conduwuit_core::event::Event` trait in favour of `ruma::state_res::Event`.
- [ ] Centralise PDU struct creation in a way that allows us to greater control the deserialization process.
    - Developers can't be trusted to read documentation so we can't store the PDU metadata (things like event ID and
      rejection status) in the PDU struct itself, as this may end up being leaked to consumers. Instead, PDU metadata
      should be stored seprately from the PDU struct itself, but needs to be strongly associated with it (i.e. fetched
      at the same time, always returned together, etc). This may take a form similar to
      `StoredPdu<T: Event> { pdu: T, metadata: PduMetadata }`.
- [ ] Stop storing generated data in the `unsigned` field, but instead calculate it on demand(?).
    - Storing generated data in the `unsigned` field results in:
        - History visibility cannot be enforced on `prev_content` or `redacted_because`.
        - `age` is manually calculated but only sometimes.
        - `membership` is not provided at all.
        - `prev_content` can leak redacted state event content ([#1103]).
        - `prev_content` cannot easily be populated for backfilled events, leaving them contextless.
        - It is easy to accidentally leak `transaction_id`.

Once `PduEvent` is gone, we can then do the following lovely things:

- Drop our local state resolution logic and event authorisation logic and instead use Ruma's, which naturally receives
  more love and attention. Our current `Event` trait and `PduEvent` implementations are incompatible with what Ruma
  needs, and do not store the appropriate information required.
- There's something else but I forgot just trust me
