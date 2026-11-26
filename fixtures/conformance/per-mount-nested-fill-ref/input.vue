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
const tag = ref("in");
let mark;
</script>
<template>
  <Panel v-if="on" title="x">
    <template #header>
      <div><slot name="inner"><em class="none">{{ tag }}</em></slot></div>
    </template>
    <template #inner><b ref="mark">{{ tag }}</b></template>
  </Panel>
</template>
