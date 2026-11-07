<script setup lang="ts">
defineOptions({ name: "Badge" });
const { count } = defineProps<{ count: string }>();
</script>
<template>
  <b class="badge">{{ count }}</b>
</template>
<script setup lang="ts">
defineOptions({ name: "Panel" });
const { title } = defineProps<{ title: string }>();
</script>
<template>
  <section class="panel">
    <h2>{{ title }}</h2>
    <slot name="header" />
    <slot />
  </section>
</template>
<script setup lang="ts">
defineOptions({ name: "App" });
const n = ref("two");
</script>
<template>
  <Panel title="Status">
    <template #header><Badge :count="n" /></template>
    <p>grandchild mounted into the fill</p>
  </Panel>
</template>
