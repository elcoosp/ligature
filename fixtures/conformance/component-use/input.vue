<script setup lang="ts">
defineOptions({ name: "Panel" });

const { title } = defineProps<{ title: string }>();
</script>

<template>
  <section class="panel">
    <h1>{{ title }}</h1>
    <slot />
  </section>
</template>
<script setup lang="ts">
defineOptions({ name: "App" });
</script>

<template>
  <Panel title="Status">
    <p>all green</p>
  </Panel>
</template>
