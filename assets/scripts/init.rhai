
fn map.make_job(name) {
    if "job" !in this {
        this.job = #{};
    }
    this.job[name] = #{};
    this.job[name].population = 0.0;
    this.job[name].dpopulation = 0.0;
    this.job[name].productivity = 0.0;
    this.job[name].specific_prod = 0.0;
    this.job[name].happiness = 0.0;
    this.job[name].commute = 0.0;
    this.job[name].demand = 0.0;
}


data.make_job("collecter");
data.make_job("researcher");
data.make_job("crafter");
data.make_job("teacher");
data.make_job("builder");
data.make_job("artist");


data.stat = #{};

data.stat.fame = 0.0;
data.stat.dfame = 0.0;
data.stat.migration = 0.0;
data.stat.natality = 0.0;
data.stat.science = 0.0;
data.stat.idleness = 0.0;

data.resource = #{};
data.resource.money = 0.0;
data.resource.dmoney = 0.0;
data.resource.food = 0.0;
data.resource.dfood = 0.0;
data.resource.material = 0.0;
data.resource.dmaterial = 0.0;

data.aggregates = #{};
data.aggregates.population = 0.0;
data.aggregates.avg_productivity = 0.0;
data.aggregates.avg_demand = 0.0;
data.aggregates.avg_happiness = 0.0;
data.aggregates.avg_commute = 0.0;

data.building = #{};
data.building.farm_area = 0.0;
data.building.mine_area = 0.0;
data.building.work_area = 0.0;
data.building.habitations = 0.0;

data.aims = #{};
data.aims.happiness = 1.;
data.aims.productivity = 1.;

//Some init values
data.job.collecter.population = 1.;
data.job.collecter.productivity = 1.;
data.aggregates.population = 1.;
data.aggregates.avg_happiness = 1.;
data.aggregates.avg_productivity = 1.;
data.aggregates.avg_demand = 1.;
data.resource.food = 1.;
data.resource.food_spoilage = 0.98;

data.stat.death_rate = 0.99;
data.building.habitations = 1000.0;