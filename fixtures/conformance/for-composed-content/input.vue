<script setup lang="ts">
import { ref } from "vue";

defineOptions({ name: "PickList" });

const on = ref(true);
const rows = ref(["alpha", "beta"]);
const picked = ref("");
</script>

<template>
  <Cell v-if="on">
    <template #cell>
      <li v-for="r in rows" :key="r" @click="picked = r">
        <slot name="row"><i class="pick">{{ r }}</i></slot>
      </li>
    </template>
  </Cell>
</template>
<script setup lang="ts">
defineOptions({ name: "Cell" });
</script>

<template>
  <slot name="cell" />
</template>
