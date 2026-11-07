<script setup lang="ts">
defineOptions({ name: "Panel" });
const { title } = defineProps<{ title: string }>();
</script>
<template>
  <section class="panel">
    <h2>{{ title }}</h2>
    <slot name="header" />
  </section>
</template>
<script setup lang="ts">
defineOptions({ name: "App" });
const tag = ref("in");
</script>
<template>
  <div>
    <Panel title="Status">
      <template #header>
        <div>
          <slot name="inner"><em class="none">{{ tag }}</em></slot>
        </div>
      </template>
      <template #inner><b class="hl">{{ tag }}</b></template>
    </Panel>
    <Panel title="Bare">
      <template #header>
        <div>
          <slot name="inner"><em class="none">{{ tag }}</em></slot>
        </div>
      </template>
    </Panel>
  </div>
</template>
