<script setup lang="ts">
defineOptions({ name: "Panel" });

const { title } = defineProps<{ title: string }>();
</script>

<template>
  <section class="panel">
    <h2>{{ title }}</h2>
    <slot name="header" />
    <slot name="foot"><i class="fb">{{ title }}</i></slot>
  </section>
</template>
<script setup lang="ts">
defineOptions({ name: "App" });
const label = ref("live");
</script>
<template>
  <Panel title="Status">
    <template #header>
      <button @click="label = 'next'">{{ label }}</button>
    </template>
  </Panel>
</template>
