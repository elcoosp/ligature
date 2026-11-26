<script setup lang="ts">
defineOptions({ name: "Panel" });
const { title } = defineProps<{ title: string }>();
</script>
<template>
  <section>
    <h2>{{ title }}</h2>
    <slot name="header" />
  </section>
</template>
<script setup lang="ts">
defineOptions({ name: "App" });
const on = ref(true);

let mark;
</script>
<template>
  <Panel v-if="on" title="x">
    <template #header>
      <b ref="mark">in</b>
    </template>
  </Panel>
</template>
