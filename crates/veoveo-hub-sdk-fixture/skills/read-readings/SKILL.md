---
name: read-readings
description: Read Observatory's sensor readings, one at a time or as a list.
metadata:
  audience: test
---
# Read readings

`observatory__readings` lists the readings a caller may see; each reading is also the
resource `observatory://readings/{id}`. See `references/units.md` for what a value means.
`observatory__search_knowledge` finds readings, the collection `observatory.readings`,
by the words of their identifiers; read each hit's address for the reading.
