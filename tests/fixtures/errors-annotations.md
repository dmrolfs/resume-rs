<!-- ABOUTME: Fixture with every kind of annotation error: bad tokens, placement, and alternate groups. -->
<!-- ABOUTME: Each annotation that is wrong, or does not clearly mark one item, is a located error. -->

# Ada Lovelace

London · ada@example.com

## Experience <!-- =experience.default -->

### Acme · Engineer · 2015 – 2017

- Led the platform. <!-- @cor -->
- Built the gateway. <!-- #AI -->
- Shipped it. <!-- @core @extra -->
- Ran <!-- #ai --> the team.

<!-- #ai -->
<!-- @core -->
- Two annotations above one bullet.

<!-- #data -->

- A blank line under the annotation above.

- The last bullet.
  <!-- #indented -->

## Groups

- First wording. <!-- =first.long -->
- Second wording. <!-- =first.short -->
- Same name twice. <!-- =twice.default -->
- Same name twice again. <!-- =twice.default -->
- Core wording. <!-- =prio.default @core -->
- Extra wording. <!-- =prio.short @extra -->
- Apart, default. <!-- =apart.default -->
- In between.
- Apart, short. <!-- =apart.short -->
- Reused, default. <!-- =reused.default -->
- Reused, short. <!-- =reused.short -->

Between the groups.

- Reused again. <!-- =reused.default -->
## Ids
- Named once. <!-- &named -->
- Named again. <!-- &named -->
- A wording with an id. <!-- =worded.default &worded -->
- Another wording. <!-- =worded.short -->
- An id that is a group name. <!-- &reused -->
