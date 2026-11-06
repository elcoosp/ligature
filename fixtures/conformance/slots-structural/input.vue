<script setup lang="ts">
import { ref } from "vue";

defineOptions({ name: "Panel" });

const { title } = defineProps<{ title: string }>();
</script>

<template>
  <section class="panel">
    <h2>{{ title }}</h2>
    <slot name="header" />
    <slot name="foot">
      <div>
        <i v-if="title === 'Status'" class="fb">{{ title }}</i>
      </div>
    </slot>
  </section>
</template>
<script setup lang="ts">
import { ref } from "vue";

defineOptions({ name: "App" });

const on = ref(true);
const label = ref("live");
const rows = ref(["alpha", "beta"]);
</script>
<template>
  <Panel title="Status">
    <template #header>
      <div>
        <b v-if="on" class="hl">{{ label }}</b>
        <ul>
          <li v-for="t in rows" :key="t">{{ t }}</li>
        </ul>
      </div>
    </template>
  </Panel>
</template>
